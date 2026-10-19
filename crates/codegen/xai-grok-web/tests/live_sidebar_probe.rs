//! 对着**正在运行的**宿主问一遍侧栏要的那几个方法，把回包原样打出来。
//!
//! 单元测试能证明代码是对的，证明不了**你浏览器连着的那个进程**跑的是这份代码。
//! 界面显示「新会话 / 20736 天」这种样子，最快的判别就是直接问活的进程：它回的
//! `workspacePath` 是不是空、时间是不是 0（0 会被界面换算成「两万天前」）。
//!
//! 默认不跑：`CHAOS_WEB_PROBE_URL=ws://127.0.0.1:8787/ws/zcode cargo test -p xai-grok-web --test live_sidebar_probe -- --ignored --nocapture`

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

fn call_frame(id: u32, channel: &str, name: &str, arg: Value) -> Vec<u8> {
    write_frame(&write_message(
        &RpcValue::Json(json!([rpc_type::PROMISE, id, channel, name])),
        &RpcValue::Json(arg),
    ))
}

async fn reply(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        // 活的宿主会自己发 ping/pong 与事件帧，探针只管跳过它们。
        let Some(frame) = next_binary(socket).await else {
            panic!("宿主没有回话");
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
        if kind != rpc_type::PROMISE_SUCCESS as u64 && kind != rpc_type::PROMISE_ERROR as u64 {
            continue;
        }
        return match body {
            RpcValue::Json(value) => value,
            _ => Value::Null,
        };
    }
}

async fn next_binary(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Option<Vec<u8>> {
    for _ in 0..200 {
        match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => return Some(bytes.to_vec()),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(reason))) => panic!("连接断了：{reason}"),
            Ok(None) => return None,
            Err(_) => return None,
        }
    }
    None
}

#[tokio::test]
#[ignore = "连的是正在运行的宿主，不是测试里起的那个"]
async fn ask_the_running_host_what_the_sidebar_will_show() {
    let url = std::env::var("CHAOS_WEB_PROBE_URL")
        .unwrap_or_else(|_| "ws://127.0.0.1:8787/ws/zcode".to_string());
    let (mut socket, _) = connect_async(&url).await.expect("接不上，宿主没在跑？");
    socket
        .send(Message::Ping(vec![].into()))
        .await
        .unwrap_or_default();
    let _ = Duration::from_millis(0);

    // 侧栏真正读的是这两条订阅：`controller/workspaces`（项目分组）与
    // `controller/tasks-index`（每个项目下面的对话行）。官方形状是先
    // `EVENT_LISTEN window-controller.onDynamicControllerFrame`，再
    // `subscribeControllerV4 {topic}`，快照以 event/fire 帧推给监听者。
    // 监听帧与调用帧只差头部的类型位（102 = EVENT_LISTEN）；用调用帧发它，
    // 桥只会当成一次普通调用，监听者数为 0，快照就没人有。
    socket
        .send(Message::Binary(
            write_frame(&write_message(
                &RpcValue::Json(json!([
                    rpc_type::EVENT_LISTEN,
                    1_u32,
                    "window-controller",
                    "onDynamicControllerFrame"
                ])),
                &RpcValue::Undefined,
            ))
            .into(),
        ))
        .await
        .unwrap();
    let _ = next_binary(&mut socket).await;
    let mut id = 2_u32;
    for topic in ["controller/workspaces", "controller/tasks-index"] {
        socket
            .send(Message::Binary(
                call_frame(
                    id,
                    "window-controller",
                    "subscribeControllerV4",
                    json!({ "topic": topic }),
                )
                .into(),
            ))
            .await
            .unwrap();
        id += 1;
        let mut seen = false;
        for _ in 0..40 {
            let Some(frame) = next_binary(&mut socket).await else {
                break;
            };
            if frame.len() < 13 {
                continue;
            }
            let len = u32::from_be_bytes(frame[9..13].try_into().unwrap()) as usize;
            let Ok((_header, body)) = read_message(&frame[13..13 + len]) else {
                continue;
            };
            let RpcValue::Json(payload) = body else {
                continue;
            };
            // 推来的是一帧 `{topic, subscriptionId, payload:{kind:"snapshot", snapshot:{…}}}`：
            // topic 在帧的顶层，真正的内容在 payload.snapshot 里。
            if payload.get("topic").and_then(Value::as_str) != Some(topic) {
                continue;
            }
            let snapshot = payload
                .get("payload")
                .and_then(|p| p.get("snapshot"))
                .cloned()
                .unwrap_or(payload.clone());
            if let Some(paths) = snapshot.get("workspaces").and_then(Value::as_array) {
                eprintln!("{topic}：{} 个项目", paths.len());
                for w in paths {
                    eprintln!("  目录={}", w["workspacePath"].as_str().unwrap_or("?"));
                }
                seen = true;
                break;
            }
            if let Some(tasks) = snapshot.get("tasks").and_then(Value::as_array) {
                eprintln!("{topic}：{} 行对话", tasks.len());
                for t in tasks {
                    eprintln!(
                        "  目录={:<48} 更新={:<14} 创建={:<14} 标题={:?}",
                        t["address"]["workspacePath"]
                            .as_str()
                            .or_else(|| t["meta"]["workspacePath"].as_str())
                            .unwrap_or("<空>"),
                        t["meta"]["updatedAt"].as_u64().unwrap_or(0),
                        t["meta"]["createdAt"].as_u64().unwrap_or(0),
                        t["meta"]["title"].as_str().unwrap_or("")
                    );
                }
                seen = true;
                break;
            }
        }
        if !seen {
            eprintln!("{topic}：没收到快照帧");
        }
    }

    for (channel, name, arg) in [
        ("zcode-task", "listTasks", json!({})),
        (
            "window-controller",
            "listTaskList",
            json!({"kind": "timeline"}),
        ),
    ] {
        socket
            .send(Message::Binary(call_frame(id, channel, name, arg).into()))
            .await
            .unwrap();
        id += 1;
        let body = reply(&mut socket).await;
        let items = body
            .as_array()
            .cloned()
            .or_else(|| body.get("items").and_then(Value::as_array).cloned())
            .unwrap_or_default();
        eprintln!("{channel}/{name}：{} 行", items.len());
        for row in &items {
            eprintln!(
                "  目录={:<48} 时间={:<14} 消息={:<4} 标题={:?}",
                row["workspacePath"].as_str().unwrap_or("<空>"),
                row["updatedAt"].as_u64().unwrap_or(0),
                row["messageCount"].as_u64().unwrap_or(0),
                row["title"].as_str().unwrap_or("")
            );
        }
    }
}
