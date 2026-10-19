//! 端到端对照：真实会话库（操作者本机）里的会话，按生产路径
//! （Engine → chaos_bridge → /ws/zcode → 官方 rpc 帧）取回初始锚点帧，
//! 落盘后交给部署 bundle 里的冻结合同（zod schema）复验。
//!
//! 默认不跑（`#[ignore]`）：读的是操作者自己的 `~/.chaos/web/sessions.sqlite`
//! 副本；`cargo test -p xai-grok-web --test real_session_anchor_probe -- --ignored --nocapture`。

use std::time::Duration;

use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

fn inner(bytes: &[u8]) -> Vec<u8> {
    let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    bytes[13..13 + len].to_vec()
}

async fn next_conversation_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(15), socket.next())
            .await
            .expect("帧 15s 内到达")
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
        if items.first().and_then(Value::as_u64) != Some(u64::from(rpc_type::EVENT_FIRE)) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            continue;
        };
        if value["frame"]["topic"]
            .as_str()
            .is_some_and(|topic| topic.starts_with("conversation/"))
        {
            return value;
        }
    }
}

#[tokio::test]
#[ignore = "读真实 ~/.chaos/web/sessions.sqlite 的副本，需要操作者本机环境"]
async fn the_real_session_anchor_frame_is_dumped_for_schema_recheck() {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        eprintln!("没有 HOME，跳过");
        return;
    };
    let live = home.join(".chaos/web/sessions.sqlite");
    let session_ids: Vec<String> = std::env::var("CHAOS_PROBE_SESSIONS")
        .unwrap_or_else(|_| {
            "37dc0764-c352-4f2b-b8b4-d4e4bcdb666a,a618460b-228d-4384-ab00-c8144890c430".into()
        })
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let dir = tempfile::tempdir().expect("tempdir");
    let copy = dir.path().join("sessions.sqlite");
    std::fs::copy(&live, &copy).expect("copy live sqlite");
    let engine = Engine::with_sqlite_store(&copy).expect("engine from copy");

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(120)).await })
            .await;
    });

    for session_id in &session_ids {
        let (mut socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
            .await
            .expect("connect");
        // 官方启动序：会话主题的监听先于一切注册。
        let listen_frame = write_message(
            &RpcValue::Json(serde_json::json!([
                rpc_type::EVENT_LISTEN,
                900,
                "zcode-agent",
                "onDynamicConversationFrame"
            ])),
            &RpcValue::Undefined,
        );
        socket
            .send(Message::Binary(write_frame(&listen_frame).into()))
            .await
            .unwrap();
        let call_frame = {
            let mut frame =
                zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(serde_json::json!([
                    rpc_type::PROMISE,
                    901,
                    "zcode-task",
                    "subscribeConversationV4"
                ])));
            frame.extend_from_slice(&zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(
                serde_json::json!([{ "sessionId": session_id }]),
            )));
            frame
        };
        socket
            .send(Message::Binary(write_frame(&call_frame).into()))
            .await
            .unwrap();

        let anchor = next_conversation_frame(&mut socket).await;
        assert_eq!(
            anchor["frame"]["payload"]["kind"], "snapshot",
            "订阅的锚点必须是 snapshot"
        );
        assert_eq!(anchor["deliveryKind"], "initial", "锚点以 initial 投递");
        let out = format!("/tmp/e2e-anchor-{session_id}.json");
        std::fs::write(&out, serde_json::to_string_pretty(&anchor).unwrap()).expect("write");
        eprintln!("wrote {out}");
    }
}
