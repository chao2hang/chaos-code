//! V4 命令接线的端到端验证：stop 信封真的能停一个正在跑的轮次，
//! permission_request 交互真的能到达会话主题并被 resolveInteraction 解除。
//!
//! 引擎走演示路径（带节奏的逐块产出），所以 stop 有真实的「运行中」窗口可打；
//! 审批走 `/approve-tool ` 前缀的即时应答路径，事件原样经过桥的映射层。

use std::time::Duration;

use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

fn inner(bytes: &[u8]) -> Vec<u8> {
    let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    bytes[13..13 + len].to_vec()
}

/// 与 replay 测试同一口径：设置文件读 `$HOME`，机器状态不得决定测试结果。
fn isolate_settings_home() {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let home = tempfile::tempdir().expect("临时 HOME");
        // SAFETY: 本文件每个用例要的都是「这台机器从没设置过」这个起点，方向一致，
        // 且进程内只设这一次。
        unsafe { std::env::set_var("HOME", home.path()) };
        home
    });
}

async fn serve() -> std::net::SocketAddr {
    // 演示路径带节奏：一轮演示响应被切成 8 字节的小块、块间等待，
    // 给 stop 一个真实的「正在跑」窗口。
    serve_with(Engine::new().with_demo_pacing(Duration::from_millis(60))).await
}

async fn serve_with(engine: Engine) -> std::net::SocketAddr {
    isolate_settings_home();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(120)).await })
            .await;
    });
    address
}

async fn connect(
    address: std::net::SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .unwrap();
    socket
}

async fn call(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: u32,
    channel: &str,
    method: &str,
    arg: Value,
) {
    let mut frame = zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(serde_json::json!([
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
}

async fn listen(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: u32,
    channel: &str,
    method: &str,
) {
    let frame = write_message(
        &RpcValue::Json(serde_json::json!([
            rpc_type::EVENT_LISTEN,
            id,
            channel,
            method
        ])),
        &RpcValue::Undefined,
    );
    socket
        .send(Message::Binary(write_frame(&frame).into()))
        .await
        .unwrap();
}

/// 读下一个会话主题帧（EventFire 上的 wire 载荷），跳过与本测试无关的应答。
/// 静默 10s 视作帧流结束，返回 None——等某帧出现的用例要把「没来」写进自己的
/// 失败信息，不能在这里 panic 掉账本。
///
/// 桥端按 V4 合同把超过 1MiB 物理上限的 logical frame 切成 `fragment`
///（wire_fragmentation 模块）；官方客户端在装配器里按序重装后才看到 logical
/// frame。这里与客户端同口径：fragment 按 (topic,subId,ordinal) 归组收片，
/// 齐片重装并校验 crc32 后以 `frame` 位挂回 logical frame 返回。
async fn try_next_conversation_frame(socket: &mut Socket) -> Option<Value> {
    thread_local! {
        static PENDING: std::cell::RefCell<
            std::collections::HashMap<String, (u64, u64, String, std::collections::HashMap<u64, String>)>,
        > = std::cell::RefCell::default();
    }
    loop {
        let wire = match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(Message::Binary(wire)))) => wire,
            Ok(Some(Ok(_))) => continue,
            _ => return None,
        };
        let (header, body) = match read_message(inner(wire.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.first().and_then(Value::as_u64) != Some(u64::from(rpc_type::EVENT_FIRE)) {
            continue;
        };
        let RpcValue::Json(value) = body else {
            continue;
        };
        if value["kind"].as_str() == Some("fragment") {
            let is_conversation = value["topic"]
                .as_str()
                .is_some_and(|topic| topic.starts_with("conversation/"));
            if !is_conversation {
                continue;
            }
        } else if value["frame"]["topic"]
            .as_str()
            .is_some_and(|topic| topic.starts_with("conversation/"))
        {
            return Some(value);
        } else {
            continue;
        }
        let reassembled = PENDING.with_borrow_mut(|pending| {
            let route = format!(
                "{}\u{0}{}\u{0}{}",
                value["topic"].as_str().unwrap_or("?"),
                value["subscriptionId"].as_str().unwrap_or("?"),
                value["logicalFrameOrdinal"],
            );
            let entry = pending.entry(route).or_insert_with(|| {
                (
                    value["fragmentCount"].as_u64().unwrap_or(0),
                    value["logicalBytes"].as_u64().unwrap_or(0),
                    value["checksum"]["value"].as_str().unwrap_or("").to_owned(),
                    std::collections::HashMap::new(),
                )
            });
            entry.3.insert(
                value["fragmentIndex"].as_u64().unwrap_or(0),
                value["dataBase64"].as_str().unwrap_or("").to_owned(),
            );
            if entry.3.len() as u64 != entry.0 {
                return None;
            }
            let mut logical: Vec<u8> = Vec::new();
            for index in 0..entry.0 {
                let data = entry.3.get(&index).expect("collected fragment index");
                use base64::Engine as _;
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
            Some(serde_json::json!({
                "wireVersion": value["wireVersion"],
                "kind": "complete",
                "deliveryKind": value["deliveryKind"],
                "logicalFrameId": value["logicalFrameId"],
                "logicalFrameOrdinal": value["logicalFrameOrdinal"],
                "topic": value["topic"],
                "subscriptionId": value["subscriptionId"],
                "frame": logical_frame,
            }))
        });
        if let Some(logical) = reassembled {
            return Some(logical);
        }
    }
}

async fn next_conversation_frame(socket: &mut Socket) -> Value {
    try_next_conversation_frame(socket)
        .await
        .expect("帧 10s 内到达")
}

/// 取一条 deltas 帧的 ops；帧流静默则返回 None。
async fn next_ops_or_silence(socket: &mut Socket) -> Option<Vec<Value>> {
    loop {
        let wire = try_next_conversation_frame(socket).await?;
        if wire["frame"]["payload"]["kind"] == "deltas" {
            return Some(
                wire["frame"]["payload"]["deltas"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
}
/// 从主题帧流里取一条 deltas 帧；snapshot（锚点）被跳过。
async fn next_deltas(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        let wire = next_conversation_frame(socket).await;
        if wire["frame"]["payload"]["kind"] == "deltas" {
            return wire;
        }
    }
}

#[tokio::test]
async fn stop_envelope_interrupts_a_running_turn() {
    let address = serve().await;
    let mut socket = connect(address).await;

    // 官方启动序：会话主题的监听先于一切注册。
    listen(
        &mut socket,
        900,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;

    // 1. createTask 给出会话 id。
    call(
        &mut socket,
        901,
        "zcode-task",
        "createTask",
        serde_json::json!({ "workspacePath": std::env::temp_dir() }),
    )
    .await;
    let task_id = loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("createTask 应答到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(wire) = message else {
            continue;
        };
        let (header, body) = match read_message(inner(wire.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.get(1).and_then(Value::as_u64) != Some(901) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            panic!("createTask 应答是对象");
        };
        break value["taskId"].as_str().expect("taskId").to_owned();
    };

    // 2. 订阅会话主题，锚点快照随后到达。
    call(
        &mut socket,
        902,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": task_id }),
    )
    .await;
    let anchor = next_conversation_frame(&mut socket).await;
    assert_eq!(anchor["frame"]["payload"]["kind"], "snapshot");
    // stop 已接线：锚点如实亮停止按钮（此刻还没在跑，应为 false）。
    assert_eq!(
        anchor["frame"]["payload"]["snapshot"]["control"]["canStop"],
        false
    );

    // 3. 长提示词让演示轮次跑一会儿，然后确认流式真的在出字。
    let long_prompt = "请把下面这段话慢慢复述一遍。".repeat(8);
    call(
        &mut socket,
        903,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": "cmd-send",
                "type": "sendText",
                "sessionId": task_id,
                "payload": { "text": long_prompt },
            },
        }),
    )
    .await;
    let mut saw_streaming_text = false;
    let mut deltas = next_deltas(&mut socket).await;
    for _ in 0..8 {
        let has_chunk = deltas["frame"]["payload"]["deltas"]
            .as_array()
            .is_some_and(|ops| {
                ops.iter().any(|op| {
                    op["op"] == "row.delta" && op["path"] == "text"
                        || op["op"] == "row.appended" && op["row"]["kind"] == "assistantText"
                })
            });
        if has_chunk {
            saw_streaming_text = true;
            break;
        }
        deltas = next_deltas(&mut socket).await;
    }
    assert!(saw_streaming_text, "演示轮次的文本块应先于 stop 到达");

    // 4. 官方停止动词：sendConversationCommandV4 + envelope type "stop"。
    call(
        &mut socket,
        904,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "envelope": {
                "commandId": "cmd-stop",
                "type": "stop",
                "sessionId": task_id,
                "payload": {},
            },
        }),
    )
    .await;

    // 5. 轮次以 interrupted 收场：控制面相位 completedInterrupted，行状态 interrupted。
    let mut saw_interrupted = false;
    for _ in 0..20 {
        let deltas = next_deltas(&mut socket).await;
        let ops = deltas["frame"]["payload"]["deltas"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let hit = ops.iter().any(|op| {
            op["op"] == "state.updated" && op["patch"]["control"]["phase"] == "completedInterrupted"
        });
        if hit {
            saw_interrupted = true;
            break;
        }
    }
    assert!(
        saw_interrupted,
        "stop 之后必须看到 completedInterrupted 终局"
    );
}

#[tokio::test]
async fn permission_request_reaches_the_topic_and_resolves() {
    let address = serve().await;
    let mut socket = connect(address).await;

    listen(
        &mut socket,
        910,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;

    call(
        &mut socket,
        911,
        "zcode-task",
        "createTask",
        serde_json::json!({ "workspacePath": std::env::temp_dir() }),
    )
    .await;
    let task_id = loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("createTask 应答到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(wire) = message else {
            continue;
        };
        let (header, body) = match read_message(inner(wire.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.get(1).and_then(Value::as_u64) != Some(911) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            panic!("createTask 应答是对象");
        };
        break value["taskId"].as_str().expect("taskId").to_owned();
    };

    call(
        &mut socket,
        912,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": task_id }),
    )
    .await;
    let anchor = next_conversation_frame(&mut socket).await;
    assert_eq!(anchor["frame"]["payload"]["kind"], "snapshot");

    // 1. 提交一条 /approve-tool：引擎的 ToolApprovalRequested 经桥映射成
    //    pendingInteractions 里的 permission 交互，到达会话主题。
    call(
        &mut socket,
        913,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": "cmd-approve",
                "type": "sendText",
                "sessionId": task_id,
                "payload": { "text": "/approve-tool 删除过期日志" },
            },
        }),
    )
    .await;

    let mut interaction_id = String::new();
    for _ in 0..20 {
        let deltas = next_deltas(&mut socket).await;
        let ops = deltas["frame"]["payload"]["deltas"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for op in &ops {
            if op["op"] == "state.updated"
                && let Some(list) = op["patch"]["pendingInteractions"].as_array()
                && let Some(entry) = list.first()
            {
                assert_eq!(entry["payload"]["kind"], "permission");
                assert!(
                    entry["payload"]["options"]
                        .as_array()
                        .is_some_and(|options| !options.is_empty())
                );
                interaction_id = entry["interactionId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
            }
        }
        if !interaction_id.is_empty() {
            break;
        }
    }
    assert!(!interaction_id.is_empty(), "审批交互必须到达会话主题");

    // 2. resolveInteraction 拒绝并带自由文本理由：ack accepted + resolvedBy，
    //    随后 pendingInteractions 清空。
    call(
        &mut socket,
        914,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "envelope": {
                "commandId": "cmd-resolve",
                "type": "resolveInteraction",
                "sessionId": task_id,
                "clientId": "client-1",
                "payload": {
                    "interactionId": interaction_id,
                    "answer": { "optionId": "reject", "freeText": "这个目录不要动" },
                },
            },
        }),
    )
    .await;

    let mut saw_ack = false;
    let mut saw_cleared = false;
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("应答到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(wire) = message else {
            println!("[dbg] non-binary frame {message:?}");
            continue;
        };
        let (header, body) = match read_message(inner(wire.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.get(1).and_then(Value::as_u64) == Some(914) {
            let RpcValue::Json(ref value) = body else {
                panic!("resolveInteraction 应答是对象");
            };
            assert_eq!(value["status"], "accepted");
            assert_eq!(value["result"]["type"], "resolveInteraction");
            assert_eq!(value["result"]["resolvedBy"]["clientId"], "client-1");
            assert_eq!(value["result"]["resolvedBy"]["optionId"], "reject");
            saw_ack = true;
        }
        if items.first().and_then(Value::as_u64) == Some(u64::from(rpc_type::EVENT_FIRE)) {
            let RpcValue::Json(ref value) = body else {
                continue;
            };
            if value["frame"]["payload"]["kind"] == "deltas" {
                let ops = value["frame"]["payload"]["deltas"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if ops.iter().any(|op| {
                    op["op"] == "state.updated"
                        && op["patch"]["pendingInteractions"]
                            .as_array()
                            .is_some_and(|list| list.is_empty())
                }) {
                    saw_cleared = true;
                }
            }
        }
        if saw_ack && saw_cleared {
            break;
        }
    }
    assert!(saw_cleared, "resolve 之后 pendingInteractions 必须清空");
}

/// 两回合各出两段思考 + 一段正文的假 Agent（生产同款 HeadlessProcessAdapter
/// 路径）。思考拆成两行 stdout，是为了让「一行流式」与「每块各建一行」可区分。
fn write_scripted_agent(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("fake-agent.sh");
    std::fs::write(
        &script,
        concat!(
            "#!/usr/bin/env bash\n",
            "prompt=\"${@: -1}\"\n",
            "if [[ \"$prompt\" == *\"看看\"* ]]; then\n",
            "  printf '%s\\n' '{\"type\":\"thought\",\"data\":\"想想\"}'\n",
            "  printf '%s\\n' '{\"type\":\"thought\",\"data\":\"再想想\"}'\n",
            "  printf '%s\\n' '{\"type\":\"text\",\"data\":\"第二轮的回答\"}'\n",
            "else\n",
            "  printf '%s\\n' '{\"type\":\"thought\",\"data\":\"先想一半\"}'\n",
            "  printf '%s\\n' '{\"type\":\"thought\",\"data\":\"再想一半\"}'\n",
            "  printf '%s\\n' '{\"type\":\"text\",\"data\":\"第一轮的回答\"}'\n",
            "fi\n",
            "sleep 0.05\n",
            "printf '%s\\n' '{\"type\":\"usage\",\"usage\":{\"inputTokens\":120,\"outputTokens\":45,\"reasoningTokens\":8,\"cache_read_input_tokens\":24,\"cache_creation_input_tokens\":6}}'\n",
            "printf '%s\\n' '{\"type\":\"end\",\"stopReason\":\"end_turn\",\"sessionId\":\"s\",\"requestId\":\"r\"}'\n",
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

async fn send_text(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    command_id: &str,
    session_id: &str,
    text: &str,
) {
    call(
        socket,
        950,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "envelope": {
                "commandId": command_id,
                "type": "sendText",
                "sessionId": session_id,
                "payload": { "text": text },
            },
        }),
    )
    .await;
}

/// 收 deltas 帧直到回合终局（control.sessionEnded），把这一轮的全部帧带走。
async fn pump_until_terminal(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Vec<Value> {
    let mut frames = Vec::new();
    loop {
        let wire = next_deltas(socket).await;
        let terminal = wire["frame"]["payload"]["deltas"]
            .as_array()
            .is_some_and(|ops| {
                ops.iter().any(|op| {
                    op["op"] == "state.updated" && op["patch"]["control"]["sessionEnded"] == true
                })
            });
        frames.push(wire);
        if terminal {
            return frames;
        }
    }
}

/// 回复不许被折进「已处理」：官方前端按 turnId 把行分组进回合单元，每个单元
/// 只保留最后一条 assistantText 展开、其余折进「已处理」。桥若把整段对话共用
/// 会话 id 当 turnId，上一轮的回复就被折没了；思考行若不随回合终局落地，回合
/// 被钉在「运行中」，回复同样选不出来。这两条合同在这里用真 provider 流钉住。
#[tokio::test]
async fn a_finished_turn_keeps_its_reply_expanded_out_of_collapsed_history() {
    let agent_dir = tempfile::tempdir().expect("临时 Agent 目录");
    let agent_binary = write_scripted_agent(agent_dir.path());
    let engine = Engine::with_adapter_arc(std::sync::Arc::new(
        chaos_engine::HeadlessProcessAdapter::new(&agent_binary, std::env::temp_dir()),
    ));
    let address = serve_with(engine).await;
    let mut socket = connect(address).await;

    listen(
        &mut socket,
        940,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;
    call(
        &mut socket,
        941,
        "zcode-task",
        "createTask",
        serde_json::json!({ "workspacePath": std::env::temp_dir() }),
    )
    .await;
    let task_id = loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("createTask 应答到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(wire) = message else {
            continue;
        };
        let (header, body) = match read_message(inner(wire.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.get(1).and_then(Value::as_u64) != Some(941) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            panic!("createTask 应答是对象");
        };
        break value["taskId"].as_str().expect("taskId").to_owned();
    };
    call(
        &mut socket,
        942,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": task_id }),
    )
    .await;
    let anchor = next_conversation_frame(&mut socket).await;
    assert_eq!(anchor["frame"]["payload"]["kind"], "snapshot");

    // ---- 第一回合：hi → 思考两段 + 回答 ----
    send_text(&mut socket, "cmd-turn-1", &task_id, "hi").await;
    let turn1 = pump_until_terminal(&mut socket).await;
    let ops: Vec<Value> = turn1
        .iter()
        .flat_map(|frame| {
            frame["frame"]["payload"]["deltas"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    // The very first live turn frame must preserve the new assistant row as
    // streaming. Settling it in the same frame as row.appended makes the V4
    // renderer classify it as work history while the turnHeader is still
    // running; only a refreshed snapshot then promotes it to visible answer.
    let start_ops = turn1[0]["frame"]["payload"]["deltas"]
        .as_array()
        .expect("sendText immediately emits the live turn frame");
    let live_reply = start_ops
        .iter()
        .find(|op| op["op"] == "row.appended" && op["row"]["kind"] == "assistantText")
        .expect("live turn frame appends assistant row");
    assert_eq!(live_reply["row"]["state"], "streaming");
    assert!(
        !start_ops.iter().any(|op| {
            op["op"] == "row.upserted" && op["row"]["rowId"] == live_reply["row"]["rowId"]
        }),
        "new live reply must not be settled before the first assistant delta: {start_ops:?}"
    );
    let appended = |kind: &str| -> Vec<Value> {
        ops.iter()
            .filter(|op| op["op"] == "row.appended" && op["row"]["kind"] == kind)
            .map(|op| op["row"].clone())
            .collect()
    };
    // 回合按 turnId 分组：第一回合的行都是 turn-1。
    for row in appended("userInput") {
        assert_eq!(row["turnId"], "turn-1", "userInput 行归第一回合");
    }
    for row in appended("assistantText") {
        assert_eq!(row["turnId"], "turn-1", "回复行归第一回合");
    }
    // 思考是一行流式：两个思考块只建一行，第二块走 row.delta 追加。
    let reasoning_rows = appended("reasoning");
    assert_eq!(reasoning_rows.len(), 1, "两个思考块不许建两行");
    assert_eq!(reasoning_rows[0]["state"], "streaming");
    assert_eq!(reasoning_rows[0]["turnId"], "turn-1");
    let thought_row_id = reasoning_rows[0]["rowId"].clone();
    let thought_deltas: Vec<&Value> = ops
        .iter()
        .filter(|op| {
            op["op"] == "row.delta" && op["path"] == "text" && op["rowId"] == thought_row_id
        })
        .collect();
    assert!(!thought_deltas.is_empty(), "第二个思考块应追加到同一思考行");
    // 终局：思考行与回复行都落地成 complete——思考行不落地，前端就把回合
    // 钉在「运行中」，回复选不出来当可见文本。
    let upserted = |kind: &str| -> Vec<Value> {
        ops.iter()
            .filter(|op| op["op"] == "row.upserted" && op["row"]["kind"] == kind)
            .map(|op| op["row"].clone())
            .collect()
    };
    let thought_settled = upserted("reasoning");
    assert_eq!(thought_settled.len(), 1, "思考行随回合终局落地一次");
    assert_eq!(thought_settled[0]["state"], "complete");
    assert_eq!(
        thought_settled[0]["text"].as_str().unwrap(),
        format!("先想一半{}", "再想一半"),
        "思考块的文本都落在同一行"
    );
    let reply_settled = upserted("assistantText");
    let terminal_reply = reply_settled
        .iter()
        .find(|row| row["state"] == "complete")
        .expect("回复最终必须带 complete 状态");
    assert_eq!(terminal_reply["text"], "第一轮的回答");
    let usage = &terminal_reply["usage"];
    assert_eq!(usage["inputTokens"], 120);
    assert_eq!(usage["outputTokens"], 45);
    assert_eq!(usage["cachedTokens"], 24);
    assert_eq!(usage["cacheCreationTokens"], 6);
    assert_eq!(usage["cacheTotalTokens"], 150);
    assert!(
        usage["outputTokensPerSecond"]
            .as_f64()
            .is_some_and(|rate| rate > 0.0),
        "finished reply row should carry measured decode rate: {usage}"
    );
    assert!(
        reply_settled
            .iter()
            .all(|row| row["state"] != "streaming" || row["text"] == "第一轮的回答"),
        "终局后的 usage 只能更新终态行，不能回退成空 streaming 行: {reply_settled:?}"
    );

    // 回合头行：开场 running + startedAt，终局 completedSuccess + endedAt。
    // 前端按它显示「已处理/已工作 {时长}」并把回合是否在跑锚在它的 state 上。
    let header_rows = appended("turnHeader");
    assert_eq!(header_rows.len(), 1, "回合开场只建一个头行");
    assert_eq!(header_rows[0]["state"], "running");
    assert_eq!(header_rows[0]["turnId"], "turn-1");
    assert!(
        header_rows[0]["startedAt"].as_u64().is_some(),
        "头行带 startedAt 供时长计算"
    );
    let header_settled = upserted("turnHeader");
    assert_eq!(header_settled.len(), 1, "头行随回合终局落地一次");
    assert_eq!(header_settled[0]["state"], "completedSuccess");
    assert_eq!(header_settled[0]["rowId"], header_rows[0]["rowId"]);
    assert!(
        header_settled[0]["endedAt"].as_u64().unwrap()
            >= header_rows[0]["startedAt"].as_u64().unwrap(),
        "endedAt 不早于 startedAt，前端才有真实时长"
    );

    // ---- 第二回合：新回合号，回复不与上一回合并组 ----
    send_text(&mut socket, "cmd-turn-2", &task_id, "看看这个项目").await;
    let turn2 = pump_until_terminal(&mut socket).await;
    let ops2: Vec<Value> = turn2
        .iter()
        .flat_map(|frame| {
            frame["frame"]["payload"]["deltas"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let appended2 = |kind: &str| -> Vec<Value> {
        ops2.iter()
            .filter(|op| op["op"] == "row.appended" && op["row"]["kind"] == kind)
            .map(|op| op["row"].clone())
            .collect()
    };
    for row in appended2("userInput") {
        assert_eq!(row["turnId"], "turn-2", "第二回合开新号");
    }
    for row in appended2("assistantText") {
        assert_eq!(row["turnId"], "turn-2");
    }
    let reply2 = upserted2(&ops2, "assistantText");
    assert_eq!(reply2["state"], "complete");
    assert_eq!(reply2["text"], "第二轮的回答");
    let header2 = upserted2(&ops2, "turnHeader");
    assert_eq!(header2["turnId"], "turn-2");
    assert_eq!(header2["state"], "completedSuccess");

    // usage 增量更新：必须携带 contextWindow（nullable），否则官方 Zod schema 拒收整条帧。
    let usage_patch = ops2
        .iter()
        .find_map(|op| {
            if op["op"] == "state.updated" {
                op["patch"].get("usage")
            } else {
                None
            }
        })
        .expect("轮次终局前必须收到 usage 增量更新");
    assert!(
        usage_patch.get("contextWindow").is_some(),
        "usage 补丁必须带 contextWindow，否则官方 usageSchema 报 proto.frameAssemblyInvalidPayload"
    );
    assert_eq!(usage_patch["cumulative"]["inputTokens"], 240);
}

fn upserted2(ops: &[Value], kind: &str) -> Value {
    ops.iter()
        .rev()
        .find(|op| op["op"] == "row.upserted" && op["row"]["kind"] == kind)
        .expect("终局必须带该行的 upsert")["row"]
        .clone()
}

/// 订阅必须总是完成「ack → 锚点」握手。引擎答不上（会话不存在/未导入/工作区
/// 已归档，典型是客户端持有重启前的陈旧侧栏缓存）也要给诚实的空面板锚点；
/// 只发 ack 不发锚点，客户端装配器永远等不到有效帧，报
/// fault.subscription.recoveryFrameTimedOut，归档等一切后续命令全被堵死。
#[tokio::test]
async fn a_subscription_to_a_missing_session_still_anchors() {
    let address = serve().await;
    let mut socket = connect(address).await;

    listen(
        &mut socket,
        960,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;

    let missing_id = Uuid::new_v4().to_string();
    call(
        &mut socket,
        961,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": missing_id }),
    )
    .await;
    let anchor = next_conversation_frame(&mut socket).await;
    assert_eq!(anchor["frame"]["payload"]["kind"], "snapshot");
    let snapshot = &anchor["frame"]["payload"]["snapshot"];
    assert_eq!(snapshot["control"]["phase"], "error");
    assert_eq!(
        snapshot["control"]["lastError"]["code"],
        "session_not_found"
    );
    assert_eq!(snapshot["rows"]["totalCount"], 0);

    // 恢复路径同一口径：gap → resync 也必须拿到锚点，装配器才能重建。
    let subscription_id = format!("chaos-{}", Uuid::new_v4());
    call(
        &mut socket,
        962,
        "zcode-agent",
        "resyncConversationV4",
        serde_json::json!({
            "sessionId": missing_id,
            "subscriptionId": subscription_id,
        }),
    )
    .await;
    let resync_anchor = next_conversation_frame(&mut socket).await;
    assert_eq!(resync_anchor["frame"]["payload"]["kind"], "snapshot");
    assert_eq!(
        resync_anchor["frame"]["subscriptionId"], subscription_id,
        "resync 的锚点必须回到请求的订阅上"
    );
    assert_eq!(
        resync_anchor["deliveryKind"], "recovery",
        "resync 锚点的 deliveryKind 必须是 recovery，否则前端装配器当陈旧帧丢弃并报 recoveryFrameTimedOut"
    );
    assert!(
        resync_anchor["logicalFrameOrdinal"].as_u64().unwrap_or(0) >= 1,
        "logicalFrameOrdinal 必须有效"
    );
}

async fn read_reply(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_id: u64,
) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("应答到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(wire) = message else {
            continue;
        };
        let (header, body) = match read_message(inner(wire.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.get(1).and_then(Value::as_u64) != Some(expected_id) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            panic!("应答是 JSON 对象");
        };
        return value;
    }
}

#[tokio::test]
async fn send_text_before_subscribe_still_streams_and_completes() {
    let address = serve().await;
    let mut socket = connect(address).await;
    listen(
        &mut socket,
        900,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;

    // 1. V4 createSession
    call(
        &mut socket,
        1,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": "cmd-create",
                "type": "createSession",
                "payload": {},
            },
        }),
    )
    .await;
    let create_ack = read_reply(&mut socket, 1).await;
    let session_id = create_ack["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    // 2. V4 sendText immediately
    call(
        &mut socket,
        2,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": "cmd-send",
                "type": "sendText",
                "sessionId": session_id,
                "payload": { "text": "请把下面这段话慢慢复述一遍。".repeat(4) },
            },
        }),
    )
    .await;
    let send_ack = read_reply(&mut socket, 2).await;
    assert_eq!(send_ack["status"], "accepted");

    // 3. Now subscribe!
    call(
        &mut socket,
        3,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": session_id }),
    )
    .await;
    let sub_ack = read_reply(&mut socket, 3).await;
    let sub_id = sub_ack["ack"]["subscriptionId"].as_str().unwrap();

    // 4. Anchor snapshot arrives
    let anchor = next_conversation_frame(&mut socket).await;
    assert_eq!(anchor["frame"]["payload"]["kind"], "snapshot");
    assert_eq!(anchor["subscriptionId"], sub_id);

    // 5. Subsequent deltas arrive with matching subscriptionId!
    let mut saw_chunk = false;
    for _ in 0..15 {
        let frame = next_conversation_frame(&mut socket).await;
        if frame["frame"]["payload"]["kind"] == "deltas" {
            saw_chunk = true;
            break;
        }
    }
    assert!(saw_chunk, "应当收到后续的流式增量帧");
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 官方启动序：监听会话主题 → createTask → subscribeConversationV4 → 读锚点。
async fn open_conversation(socket: &mut Socket, listen_id: u32, subscribe_id: u32) -> String {
    listen(
        socket,
        listen_id,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;
    call(
        socket,
        listen_id + 1,
        "zcode-task",
        "createTask",
        serde_json::json!({ "workspacePath": std::env::temp_dir() }),
    )
    .await;
    let created = read_reply(socket, u64::from(listen_id + 1)).await;
    let task_id = created["taskId"]
        .as_str()
        .expect("createTask 带 taskId")
        .to_owned();
    call(
        socket,
        subscribe_id,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": task_id }),
    )
    .await;
    let anchor = next_conversation_frame(socket).await;
    assert_eq!(anchor["frame"]["payload"]["kind"], "snapshot");
    task_id
}

/// 发一条 sendText 并等到流式正文出现——这正是「轮次在跑」的客户端事实。
async fn send_text_until_streaming(socket: &mut Socket, rpc_id: u32, session_id: &str, text: &str) {
    call(
        socket,
        rpc_id,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": format!("cmd-{rpc_id}"),
                "type": "sendText",
                "sessionId": session_id,
                "payload": { "text": text },
            },
        }),
    )
    .await;
    for _ in 0..15 {
        let deltas = next_deltas(socket).await;
        let streaming = deltas["frame"]["payload"]["deltas"]
            .as_array()
            .is_some_and(|ops| {
                ops.iter().any(|op| {
                    op["op"] == "row.delta" && op["path"] == "text"
                        || op["op"] == "row.appended" && op["row"]["kind"] == "assistantText"
                })
            });
        if streaming {
            return;
        }
    }
    panic!("演示轮次应先出字");
}

/// 发一条 `sendConversationCommandV4` 信封（队列管理动词走这条）。
async fn send_envelope(socket: &mut Socket, rpc_id: u32, envelope: Value) {
    call(
        socket,
        rpc_id,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({ "envelope": envelope }),
    )
    .await;
}

/// 取一条 deltas 帧的 ops。
async fn next_ops(socket: &mut Socket) -> Vec<Value> {
    next_deltas(socket).await["frame"]["payload"]["deltas"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// 收 deltas 帧直到某个 `state.updated{queue}` 补丁满足条件，返回该队列状态。
/// `label` 只进失败信息，用来区分同一条测试里的几次等待。
async fn wait_for_queue_patch(
    socket: &mut Socket,
    label: &str,
    mut matches: impl FnMut(&Value) -> bool,
) -> Value {
    let mut seen = Vec::new();
    for _ in 0..60 {
        // 帧流静默即「不会再有」：把收过的队列补丁写进失败信息，比在读取处
        // panic 掉有用。
        let Some(ops) = next_ops_or_silence(socket).await else {
            break;
        };
        for op in &ops {
            if op["op"] == "state.updated" && op["patch"]["queue"].is_object() {
                seen.push(op["patch"]["queue"].clone());
                if matches(&op["patch"]["queue"]) {
                    return op["patch"]["queue"].clone();
                }
            }
        }
    }
    panic!("{label}：期待的队列补丁没有到达会话主题，收到过的队列补丁：{seen:?}");
}

/// 运行中再发一条：引擎把它入队（不再 `session_busy` 拒绝），宿主必须把队列
/// 状态推到会话主题。前端的队列面板只在 `queue.items` 非空时渲染，补丁不来
/// 面板永远不出现——「消息队列功能在 web 端没有」的形状。
#[tokio::test]
async fn a_send_while_busy_pushes_the_queue_to_the_topic() {
    let address = serve().await;
    let mut socket = connect(address).await;
    let task_id = open_conversation(&mut socket, 970, 972).await;

    send_text_until_streaming(
        &mut socket,
        973,
        &task_id,
        &"请把下面这段话慢慢复述一遍。".repeat(40),
    )
    .await;

    // 运行中再发一条：ack 是 inputAccepted{delivery:"queue"}，队列项本体由引擎
    // 持有——宿主不推补丁，客户端就看不到它。
    send_envelope(
        &mut socket,
        974,
        serde_json::json!({
            "commandId": "cmd-busy-2",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "排队的问题", "requestedDelivery": "queue" },
        }),
    )
    .await;

    let queue = wait_for_queue_patch(&mut socket, "运行中入队", |queue| {
        queue["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
    .await;
    let items = queue["items"].as_array().expect("queue.items 是数组");
    assert_eq!(items.len(), 1, "此刻队列里应只有第二条：{queue:?}");
    assert_eq!(items[0]["text"], "排队的问题");
    assert_eq!(items[0]["sourceCommandId"], "cmd-busy-2");
    assert_eq!(
        items[0]["delivery"],
        serde_json::json!({ "requested": "queue", "admitted": "queue" })
    );
    assert_eq!(queue["autoDrain"], true, "入队时自动排水保持开启");
}

/// 队列管理动词（`deleteQueueItem` / `setAutoDrain`）同样走 async 帧循环直连
/// 引擎：结果不回推，面板上的行与暂停横幅就停在旧状态。先关掉自动排水，队列
/// 内容就不会被轮次排水带走，断言只依赖命令本身。
#[tokio::test]
async fn queue_management_verbs_reach_the_topic() {
    let address = serve().await;
    let mut socket = connect(address).await;
    let task_id = open_conversation(&mut socket, 980, 982).await;

    send_text_until_streaming(
        &mut socket,
        983,
        &task_id,
        &"请把下面这段话慢慢复述一遍。".repeat(40),
    )
    .await;
    send_envelope(
        &mut socket,
        984,
        serde_json::json!({
            "commandId": "cmd-queue-1",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "排队的问题", "requestedDelivery": "queue" },
        }),
    )
    .await;
    let queued = wait_for_queue_patch(&mut socket, "队列动词前置入队", |queue| {
        queue["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
    .await;
    let item_id = queued["items"][0]["queueItemId"]
        .as_str()
        .expect("队列项带 queueItemId")
        .to_owned();

    // 关掉自动排水：这一项留在队列里，后面删它就不再与轮次时序赛跑。
    send_envelope(
        &mut socket,
        985,
        serde_json::json!({
            "commandId": "cmd-drain-off",
            "type": "setAutoDrain",
            "sessionId": task_id,
            "payload": { "autoDrain": false },
        }),
    )
    .await;
    let stopped = wait_for_queue_patch(&mut socket, "关闭自动排水", |queue| {
        queue["autoDrain"] == false
    })
    .await;
    assert_eq!(
        stopped["items"].as_array().map(Vec::len),
        Some(1),
        "关掉排水不改变队列内容：{stopped:?}"
    );

    // 删除该项：面板随之清空。
    send_envelope(
        &mut socket,
        986,
        serde_json::json!({
            "commandId": "cmd-delete",
            "type": "deleteQueueItem",
            "sessionId": task_id,
            "payload": { "queueItemId": item_id },
        }),
    )
    .await;
    let cleared = wait_for_queue_patch(&mut socket, "删除排队项", |queue| {
        queue["items"]
            .as_array()
            .is_some_and(|items| items.is_empty())
    })
    .await;
    assert_eq!(cleared["autoDrain"], false, "删除不改排水开关");
}

/// 轮次结束后的自动排水：排队的那一条必须成为时间线上自己的一轮——新回合头、
/// 用户行、流式回复行都带新的 turnId。宿主在排水回合不开场（引擎的 TurnStarted
/// 被当成「宿主已开号的第二声钟」），排队的问题就只剩一段回答挂回上一回合：
/// 面板清空了，问的那句话在时间线上找不到。
#[tokio::test]
async fn a_drained_queue_item_becomes_its_own_turn_on_the_topic() {
    let address = serve().await;
    let mut socket = connect(address).await;
    let task_id = open_conversation(&mut socket, 990, 992).await;

    // 第一轮：宿主自己开号（V4StreamStart 画用户行），记下这个回合号。
    send_envelope(
        &mut socket,
        993,
        serde_json::json!({
            "commandId": "cmd-slow",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "请把下面这段话慢慢复述一遍。".repeat(40) },
        }),
    )
    .await;
    let first_turn = loop {
        let ops = next_ops(&mut socket).await;
        let opened = ops.iter().find_map(|op| {
            (op["op"] == "row.appended" && op["row"]["kind"] == "userInput")
                .then(|| op["row"]["turnId"].as_str().map(str::to_owned))
                .flatten()
        });
        if let Some(turn) = opened {
            break turn;
        }
    };

    // 运行中再发一条：进队列。
    send_envelope(
        &mut socket,
        994,
        serde_json::json!({
            "commandId": "cmd-queued",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "排队的问题", "requestedDelivery": "queue" },
        }),
    )
    .await;
    wait_for_queue_patch(&mut socket, "排水用例前置入队", |queue| {
        queue["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
    .await;

    // 第一轮终局后自动排水：收帧直到排水回合的用户行出现。演示路径的正文按块
    // 带节奏产出，所以这里要等的是「第一轮流完 + 排水回合开场」两件事。
    let mut drained_turn: Option<String> = None;
    let mut seen: Vec<Value> = Vec::new();
    let mut deltas = 0usize;
    for _ in 0..2000 {
        // 静默即终局：该出的帧（第一轮流完、排水回合开场）都已经出于前头。
        let Some(ops) = next_ops_or_silence(&mut socket).await else {
            break;
        };
        if drained_turn.is_none() {
            drained_turn = ops.iter().find_map(|op| {
                (op["op"] == "row.appended"
                    && op["row"]["kind"] == "userInput"
                    && op["row"]["text"] == "排队的问题")
                    .then(|| op["row"]["turnId"].as_str().map(str::to_owned))
                    .flatten()
            });
        }
        let found = drained_turn.is_some();
        deltas += ops.iter().filter(|op| op["op"] == "row.delta").count();
        seen.extend(ops.into_iter().filter(|op| op["op"] != "row.delta"));
        if found {
            break;
        }
    }
    let drained_turn = drained_turn.unwrap_or_else(|| {
        let summary: Vec<String> = seen
            .iter()
            .map(|op| match op["op"].as_str().unwrap_or("") {
                "row.appended" | "row.upserted" => format!(
                    "{}({},turn={},text={:?})",
                    op["op"].as_str().unwrap_or(""),
                    op["row"]["kind"].as_str().unwrap_or("?"),
                    op["row"]["turnId"].as_str().unwrap_or("?"),
                    op["row"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(20)
                        .collect::<String>(),
                ),
                other => format!(
                    "{other}:{}",
                    &op["patch"].to_string().chars().take(90).collect::<String>()
                ),
            })
            .collect();
        panic!(
            "排水回合必须开出自己的用户行（{deltas} 块正文行内）：{}",
            summary.join(" | ")
        );
    });
    assert_ne!(drained_turn, first_turn, "排水回合要新开一个回合号");

    let appended = |kind: &str| -> Vec<Value> {
        seen.iter()
            .filter(|op| op["op"] == "row.appended" && op["row"]["kind"] == kind)
            .map(|op| op["row"].clone())
            .collect()
    };
    assert!(
        appended("turnHeader")
            .iter()
            .any(|row| row["turnId"] == drained_turn.as_str()),
        "排水回合要有自己的回合头：{seen:?}"
    );
    assert!(
        appended("assistantText")
            .iter()
            .any(|row| row["turnId"] == drained_turn.as_str()),
        "排水回合要有自己的流式回复行：{seen:?}"
    );
    assert!(
        !appended("assistantText")
            .iter()
            .any(|row| row["turnId"] == first_turn.as_str()),
        "排水回合的回复不许挂回上一回合：{seen:?}"
    );
}

/// 按停止会把队列的自动排水挂起，这件事必须推到会话主题：不公告的话面板照旧
/// 按 autoDrain 画，既不显示暂停横幅也没有恢复入口——队列里那条无声钉住，
/// 用户以为它马上会跑。
#[tokio::test]
async fn a_stop_with_a_queued_item_publishes_the_pause() {
    let address = serve().await;
    let mut socket = connect(address).await;
    let task_id = open_conversation(&mut socket, 1000, 1002).await;

    send_text_until_streaming(
        &mut socket,
        1003,
        &task_id,
        &"请把下面这段话慢慢复述一遍。".repeat(40),
    )
    .await;
    send_envelope(
        &mut socket,
        1004,
        serde_json::json!({
            "commandId": "cmd-pause-queue",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "排队的问题", "requestedDelivery": "queue" },
        }),
    )
    .await;
    let queued = wait_for_queue_patch(&mut socket, "停止前入队", |queue| {
        queue["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
    .await;
    assert_eq!(queued["autoDrain"], true, "入队时还没暂停");
    assert!(queued.get("pauseReason").is_none(), "暂停前没有暂停理由");

    send_envelope(
        &mut socket,
        1005,
        serde_json::json!({
            "commandId": "cmd-stop",
            "type": "stop",
            "sessionId": task_id,
        }),
    )
    .await;
    let paused = wait_for_queue_patch(&mut socket, "按停止暂停队列", |queue| {
        queue["pauseReason"].is_string()
    })
    .await;
    assert_eq!(
        paused["pauseReason"], "stopped",
        "暂停理由要说清是停止按下的：{paused:?}"
    );
    assert_eq!(
        paused["items"].as_array().map(Vec::len),
        Some(1),
        "暂停不改队列内容：{paused:?}"
    );
    assert_eq!(
        paused["autoDrain"], false,
        "暂停要在 autoDrain 上看得出来（渲染端只在关闭画暂停横幅）：{paused:?}"
    );
}

/// 队列里排着两条时，排水要一条接一条排完，各自开自己的回合。排水口每取下一条
/// 前都要确认会话空闲：正在跑的那一轮会把队头退成 `session_busy`，取下来的那条
/// 就此消失——面板空了，时间线上却少了一问。
#[tokio::test]
async fn a_queue_of_two_drains_both_items_in_order() {
    let address = serve().await;
    let mut socket = connect(address).await;
    let task_id = open_conversation(&mut socket, 1200, 1202).await;

    send_envelope(
        &mut socket,
        1203,
        serde_json::json!({
            "commandId": "cmd-two-slow",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "请把下面这段话慢慢复述一遍。".repeat(40) },
        }),
    )
    .await;
    let first_turn = loop {
        let ops = next_ops(&mut socket).await;
        let opened = ops.iter().find_map(|op| {
            (op["op"] == "row.appended" && op["row"]["kind"] == "userInput")
                .then(|| op["row"]["turnId"].as_str().map(str::to_owned))
                .flatten()
        });
        if let Some(turn) = opened {
            break turn;
        }
    };

    for (rpc_id, command, text) in [
        (1204, "cmd-two-a", "排队的第一问"),
        (1205, "cmd-two-b", "排队的第二问"),
    ] {
        send_envelope(
            &mut socket,
            rpc_id,
            serde_json::json!({
                "commandId": command,
                "type": "sendText",
                "sessionId": task_id,
                "payload": { "text": text, "requestedDelivery": "queue" },
            }),
        )
        .await;
    }
    wait_for_queue_patch(&mut socket, "两条都进队列", |queue| {
        queue["items"].as_array().map(Vec::len) == Some(2)
    })
    .await;

    let mut turns: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<Value> = Vec::new();
    let mut deltas = 0usize;
    for _ in 0..4000 {
        let Some(ops) = next_ops_or_silence(&mut socket).await else {
            break;
        };
        for op in &ops {
            if op["op"] == "row.appended" && op["row"]["kind"] == "userInput" {
                let text = op["row"]["text"].as_str().unwrap_or("").to_owned();
                if text.starts_with("排队的第")
                    && !turns.iter().any(|(queued, _)| queued == &text)
                    && let Some(turn) = op["row"]["turnId"].as_str()
                {
                    turns.push((text, turn.to_owned()));
                }
            }
        }
        let done = turns.len() == 2;
        deltas += ops.iter().filter(|op| op["op"] == "row.delta").count();
        seen.extend(ops.into_iter().filter(|op| op["op"] != "row.delta"));
        if done {
            break;
        }
    }
    let summary: Vec<String> = seen
        .iter()
        .filter_map(|op| {
            (op["op"] == "row.appended" && op["row"]["kind"] == "userInput").then(|| {
                format!(
                    "{}(turn={},text={:?})",
                    op["row"]["kind"].as_str().unwrap_or("?"),
                    op["row"]["turnId"].as_str().unwrap_or("?"),
                    op["row"]["text"].as_str().unwrap_or(""),
                )
            })
        })
        .collect();
    assert_eq!(
        turns
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["排队的第一问", "排队的第二问"],
        "两条要按入队顺序各成一问（{deltas} 块正文行内），收到的用户行：{summary:?}"
    );
    assert!(
        turns.iter().all(|(_, turn)| turn != &first_turn),
        "排水回合都不能挂回第一轮：{turns:?}"
    );
    assert_ne!(
        turns[0].1, turns[1].1,
        "两条排队项要各开一轮，不能挤进同一个回合：{turns:?}"
    );
}

/// 此后再没有轮次终局会经过排水口；恢复只清暂停位的话，横幅消失、排队的那句
/// 话留在面板里干等——用户看到的正是「按了继续没反应」。
#[tokio::test]
async fn a_resume_on_an_idle_session_runs_the_queued_item() {
    let address = serve().await;
    let mut socket = connect(address).await;
    let task_id = open_conversation(&mut socket, 1100, 1102).await;

    send_envelope(
        &mut socket,
        1103,
        serde_json::json!({
            "commandId": "cmd-resume-slow",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "请把下面这段话慢慢复述一遍。".repeat(40) },
        }),
    )
    .await;
    let first_turn = loop {
        let ops = next_ops(&mut socket).await;
        let opened = ops.iter().find_map(|op| {
            (op["op"] == "row.appended" && op["row"]["kind"] == "userInput")
                .then(|| op["row"]["turnId"].as_str().map(str::to_owned))
                .flatten()
        });
        if let Some(turn) = opened {
            break turn;
        }
    };

    send_envelope(
        &mut socket,
        1104,
        serde_json::json!({
            "commandId": "cmd-resume-queued",
            "type": "sendText",
            "sessionId": task_id,
            "payload": { "text": "继续后排的问题", "requestedDelivery": "queue" },
        }),
    )
    .await;
    wait_for_queue_patch(&mut socket, "恢复用例前置入队", |queue| {
        queue["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
    .await;

    send_envelope(
        &mut socket,
        1105,
        serde_json::json!({
            "commandId": "cmd-resume-stop",
            "type": "stop",
            "sessionId": task_id,
        }),
    )
    .await;
    wait_for_queue_patch(&mut socket, "恢复前先暂停", |queue| {
        queue["pauseReason"].is_string()
    })
    .await;

    // 被打断的那一轮要真的收尾，恢复才落在空闲会话上。静默＝该出的帧都出完了。
    while next_ops_or_silence(&mut socket).await.is_some() {}

    send_envelope(
        &mut socket,
        1106,
        serde_json::json!({
            "commandId": "cmd-resume-auto-drain",
            "type": "setAutoDrain",
            "sessionId": task_id,
            "payload": { "autoDrain": true },
        }),
    )
    .await;

    let mut drained_turn: Option<String> = None;
    let mut emptied = false;
    let mut seen: Vec<Value> = Vec::new();
    let mut deltas = 0usize;
    for _ in 0..2000 {
        let Some(ops) = next_ops_or_silence(&mut socket).await else {
            break;
        };
        if drained_turn.is_none() {
            drained_turn = ops.iter().find_map(|op| {
                (op["op"] == "row.appended"
                    && op["row"]["kind"] == "userInput"
                    && op["row"]["text"] == "继续后排的问题")
                    .then(|| op["row"]["turnId"].as_str().map(str::to_owned))
                    .flatten()
            });
        }
        for op in &ops {
            if op["op"] == "state.updated"
                && op["patch"]["queue"]["items"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                emptied = true;
            }
        }
        let found = drained_turn.is_some();
        deltas += ops.iter().filter(|op| op["op"] == "row.delta").count();
        seen.extend(ops.into_iter().filter(|op| op["op"] != "row.delta"));
        if found {
            break;
        }
    }
    let drained_turn = drained_turn.unwrap_or_else(|| {
        let summary: Vec<String> = seen
            .iter()
            .map(|op| match op["op"].as_str().unwrap_or("") {
                "row.appended" | "row.upserted" => format!(
                    "{}({},turn={},text={:?})",
                    op["op"].as_str().unwrap_or(""),
                    op["row"]["kind"].as_str().unwrap_or("?"),
                    op["row"]["turnId"].as_str().unwrap_or("?"),
                    op["row"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(20)
                        .collect::<String>(),
                ),
                other => format!(
                    "{other}:{}",
                    &op["patch"].to_string().chars().take(90).collect::<String>()
                ),
            })
            .collect();
        panic!(
            "「继续」要把队头排成自己的一轮（{deltas} 块正文行内）：{}",
            summary.join(" | ")
        );
    });
    assert_ne!(drained_turn, first_turn, "恢复排出的回合要新开一个回合号");
    assert!(emptied, "排水要先在会话主题上把队列清空：{seen:?}");
    let appended = |kind: &str| -> Vec<Value> {
        seen.iter()
            .filter(|op| op["op"] == "row.appended" && op["row"]["kind"] == kind)
            .map(|op| op["row"].clone())
            .collect()
    };
    assert!(
        appended("turnHeader")
            .iter()
            .any(|row| row["turnId"] == drained_turn.as_str()),
        "恢复排出的回合要有自己的回合头：{seen:?}"
    );
    assert!(
        appended("assistantText")
            .iter()
            .any(|row| row["turnId"] == drained_turn.as_str()),
        "恢复排出的回合要有自己的流式回复行：{seen:?}"
    );
}

/// 辅助对话创建：从父会话派生副屏会话，返回正确的 V4 createSelectionSideSession ACK，
/// 且新会话可订阅。
#[tokio::test]
async fn create_selection_side_session_flow() {
    let address = serve().await;
    let mut socket = connect(address).await;

    // 1. 先建一个主会话
    call(
        &mut socket,
        1,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": "cmd-create-parent",
                "type": "createSession",
                "payload": {},
            },
        }),
    )
    .await;
    let create_ack = read_reply(&mut socket, 1).await;
    assert_eq!(create_ack["status"], "accepted");
    let parent_id = create_ack["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    // 2. 点击右侧栏「辅助对话」发出 createSelectionSideSession
    call(
        &mut socket,
        2,
        "zcode-agent",
        "sendConversationCommandV4",
        serde_json::json!({
            "workspacePath": std::env::temp_dir().to_string_lossy(),
            "envelope": {
                "commandId": "cmd-side-chat",
                "type": "createSelectionSideSession",
                "sessionId": parent_id,
                "payload": {},
            },
        }),
    )
    .await;
    let side_ack = read_reply(&mut socket, 2).await;
    assert_eq!(side_ack["status"], "accepted");
    assert_eq!(side_ack["result"]["type"], "createSelectionSideSession");
    let side_id = side_ack["result"]["sessionId"].as_str().unwrap().to_owned();
    assert_ne!(side_id, parent_id, "副屏会话是独立的新会话");

    // 3. 副屏挂载订阅新会话
    call(
        &mut socket,
        3,
        "zcode-agent",
        "subscribeConversationV4",
        serde_json::json!({ "sessionId": side_id }),
    )
    .await;
    let sub_ack = read_reply(&mut socket, 3).await;
    assert!(sub_ack.is_object() || sub_ack.is_null());
}
