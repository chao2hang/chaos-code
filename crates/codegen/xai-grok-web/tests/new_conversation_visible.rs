//! 「新建一个对话」必须在列表里看得见。
//!
//! 官方前端的侧栏靠 `zcode-task.listTasks` 填。之前这条路上有两处会把刚建的对话弄丢：
//! 一是 `message_count == 0` 被过滤掉（刚建、还没发第一句的对话正好是 0）；二是回包里的
//! `workspacePath` 回的是**请求里给的那个值**，请求没带就回空串，按工作区分组的界面就把
//! 这一行归到哪儿都不是的地方。

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
        &RpcValue::Json(json!([rpc_type::PROMISE, id, "zcode-task", name])),
        &RpcValue::Json(arg),
    ))
}

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
            continue;
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
async fn a_brand_new_conversation_shows_up_in_the_list() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(30)).await })
            .await;
    });

    let (mut socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .expect("ZCode 端点应当接上");

    socket
        .send(Message::Binary(
            call_frame(1, "createTask", json!({})).into(),
        ))
        .await
        .unwrap();
    let (id, ok, created) = reply(&mut socket).await;
    assert_eq!(id, 1);
    assert!(ok, "createTask 失败了：{created}");
    let task_id = created["taskId"]
        .as_str()
        .or_else(|| created.as_str())
        .unwrap_or_default()
        .to_owned();
    assert!(!task_id.is_empty(), "createTask 没回 taskId：{created}");

    // 列表这一头。答复来自引擎线程，可能要等一拍，所以只重试「还没出现」这一种情况。
    let mut listed = Value::Null;
    for attempt in 0..40_u32 {
        let id = 10 + attempt;
        socket
            .send(Message::Binary(
                call_frame(id, "listTasks", json!({})).into(),
            ))
            .await
            .unwrap();
        let (_, ok, body) = reply(&mut socket).await;
        assert!(ok, "listTasks 失败了：{body}");
        let hit = body
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["taskId"] == task_id.as_str()));
        listed = body;
        if hit {
            let row = listed
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["taskId"] == task_id.as_str())
                .unwrap();
            // 按工作区分组的界面：空串会让这一行落不进任何一组。
            assert!(
                row["workspacePath"]
                    .as_str()
                    .is_some_and(|path| !path.is_empty()),
                "回包里的 workspacePath 是空的，界面归不进任何工作区：{row}"
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "刚建的对话 {task_id} 在 listTasks 里根本不存在——它被 message_count == 0 过滤掉了：{listed}"
    );
}
