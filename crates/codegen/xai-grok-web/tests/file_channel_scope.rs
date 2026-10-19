//! `file.*` 这一族只能待在宿主报给前端的那个根里。
//!
//! 曾经不是：五个方法都直接拿调用方给的绝对路径去碰文件系统。只听 loopback 时
//! 那是「本用户本来就读得到」；`CHAOS_WEB_BIND=0.0.0.0` 之后它变成「局域网里任何
//! 拿着 token 的设备能读 `~/.chaos/config.toml`，里面是 Provider 的 api_key」。
//! 这条测试先把那扇门钉死：越界的绝对路径必须被拒，软链绕出去也不行，根内的正常读
//! 照旧能读。

use std::time::Duration;

use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

fn call_frame(id: u32, name: &str, arg: Value) -> Vec<u8> {
    write_frame(&write_message(
        &RpcValue::Json(json!([rpc_type::PROMISE, id, "file", name])),
        &RpcValue::Json(arg),
    ))
}

/// 取回下一次调用的答复：成功给 body，失败给错误对象。
async fn reply(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> (u64, bool, Value) {
    loop {
        let frame = match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => bytes,
            other => panic!("应当收到二进制帧，实际 {other:?}"),
        };
        if frame.len() < 13 {
            continue;
        }
        let len = u32::from_be_bytes(frame[9..13].try_into().unwrap()) as usize;
        let Ok((header, body)) = read_message(&frame[13..13 + len]) else {
            continue;
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        let kind = items.first().and_then(Value::as_u64).unwrap_or_default();
        let id = items.get(1).and_then(Value::as_u64).unwrap_or_default();
        if kind != rpc_type::PROMISE_SUCCESS as u64 && kind != rpc_type::PROMISE_ERROR as u64 {
            continue; // 服务端的 Initialize 等帧
        }
        let body = match body {
            RpcValue::Json(value) => value,
            RpcValue::Undefined => Value::Null,
            RpcValue::Binary(bytes, _) => json!({ "binary": bytes }),
        };
        return (id, kind == rpc_type::PROMISE_SUCCESS as u64, body);
    }
}

#[tokio::test]
async fn file_channel_stays_inside_the_served_root() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(20)).await })
            .await;
    });

    let (mut socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .expect("ZCode 端点应当接上");

    // 1. 工作区之外的绝对路径：拒，并把「哪个根」说清楚，操作者才有的可改。
    let outside = std::env::temp_dir().join(format!("chaos-file-probe-{}", std::process::id()));
    std::fs::write(&outside, "工作区之外的内容").expect("放一个探针文件");
    socket
        .send(Message::Binary(
            call_frame(
                1,
                "readTextFile",
                json!({ "path": outside.to_string_lossy() }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 1);
    assert!(!ok, "根外的 readTextFile 竟然成功了，内容都回来了：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("CHAOS_WEB_FILE_ROOT")),
        "报错得告诉操作者怎么放开：{body}"
    );

    // 2. 根目录本身也不行——readdir("/") 以前能列出 /etc。
    socket
        .send(Message::Binary(
            call_frame(2, "readdir", json!({ "path": "/" })).into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 2);
    assert!(!ok, "readdir 竟然还能列根目录：{body}");

    // 3. 软链绕过：根里的链接指向根外，按真实目标算。
    let root = std::env::current_dir().expect("测试进程的工作目录就是宿主报出的根");
    let link = root.join(format!("chaos-symlink-probe-{}", std::process::id()));
    std::os::unix::fs::symlink(&outside, &link).expect("放一个指向根外的软链");
    socket
        .send(Message::Binary(
            call_frame(3, "readTextFile", json!({ "path": link.to_string_lossy() })).into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    let _ = std::fs::remove_file(&link);
    assert_eq!(id, 3);
    assert!(!ok, "根里的软链指向根外，竟然读到了：{body}");

    // 4. 根内的正常读不受影响：这个 crate 的 Cargo.toml 就在宿主报出的根里。
    socket
        .send(Message::Binary(
            call_frame(4, "readTextFile", json!({ "path": "Cargo.toml" })).into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 4);
    assert!(ok, "根内的读取被误伤了：{body}");
    assert!(
        body["content"]
            .as_str()
            .is_some_and(|content| content.contains("[package]")),
        "读回来的不该是这个文件？{body}"
    );

    let _ = std::fs::remove_file(&outside);
}
