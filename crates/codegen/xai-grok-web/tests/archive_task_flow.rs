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
async fn archive_and_unarchive_task_flow() {
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

    // 1. Create a task
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

    // 2. Archive the task
    socket
        .send(Message::Binary(
            call_frame(
                2,
                "archiveTask",
                json!({ "taskId": task_id, "workspacePath": "/tmp" }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, archived_meta) = reply(&mut socket).await;
    assert_eq!(id, 2);
    assert!(ok, "archiveTask 失败：{archived_meta}");
    assert_eq!(archived_meta["archived"], true);

    // 3. Check listTasks: the archived task must NOT be in active tasks
    socket
        .send(Message::Binary(
            call_frame(3, "listTasks", json!({})).into(),
        ))
        .await
        .unwrap();
    let (_, ok, active_tasks) = reply(&mut socket).await;
    assert!(ok);
    let in_active = active_tasks
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["taskId"] == task_id);
    assert!(
        !in_active,
        "已归档的会话不该出现在 active 列表中: {active_tasks}"
    );

    // 4. Check listArchivedTasks: the task MUST be in archived tasks
    socket
        .send(Message::Binary(
            call_frame(4, "listArchivedTasks", json!({})).into(),
        ))
        .await
        .unwrap();
    let (_, ok, archived_tasks) = reply(&mut socket).await;
    assert!(ok);
    let in_archived = archived_tasks
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["taskId"] == task_id);
    assert!(
        in_archived,
        "已归档的会话应当在 listArchivedTasks 中: {archived_tasks}"
    );

    // 5. Unarchive the task
    socket
        .send(Message::Binary(
            call_frame(
                5,
                "unarchiveTask",
                json!({ "taskId": task_id, "workspacePath": "/tmp" }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (_, ok, unarchived_meta) = reply(&mut socket).await;
    assert!(ok);
    assert_eq!(unarchived_meta["archived"], false);

    // 6. Check listTasks again: now it should be back in active tasks
    socket
        .send(Message::Binary(
            call_frame(6, "listTasks", json!({})).into(),
        ))
        .await
        .unwrap();
    let (_, ok, active_tasks_again) = reply(&mut socket).await;
    assert!(ok);
    let in_active_again = active_tasks_again
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["taskId"] == task_id);
    assert!(in_active_again, "取消归档后应当回到 active 列表");
}
