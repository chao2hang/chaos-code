//! 终端里聊过的对话，在 ZCode 界面上必须**看得见、分得出目录、并且能接着聊**。
//!
//! 这条链的三段各自都可能悄悄断：
//! * 列表——`zcode-task.listTasks` / `window-controller.listTaskList` 的行以前把
//!   **请求里带来的** `workspacePath` 原样贴回去，`createdAt`/`updatedAt` 一律写
//!   「刚刚」；于是终端在别的目录里聊的整段消失，剩下的几行还都算在同一个目录、
//!   同一个时刻头上。
//! * 打开——界面取的是快照，而快照这条路从不读终端的转写，点开一行是空面板。
//! * 继续——以前引擎把「这一句」单独发给宿主，终端那边的会话根本不会被续上。
//!
//! 所以这里用一个假 CLI：它把**自己看到的 argv 和 cwd** 记到文件里。继续对话到底
//! 是不是 `--resume <终端会话 id>`、是不是跑在对话自己的目录里，只有这种证据算数。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chaos_engine::{Engine, HeadlessProcessAdapter};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

const FIRST: &str = "01a11392-1bd4-7f82-ab78-df12c9f1028c";
const SECOND: &str = "01b22403-2ce5-8a93-bc89-e023dae2139d";

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

/// 假 CLI 存的是它自己那个样子：`sessions/<urlencode(cwd)>/<uuid>/`。
/// 目录必须真实存在——「在原目录里恢复」这件事，只有目录真在才谈得上。
fn write_session(store: &Path, cwd: &Path, id: &str, summary: Value, turns: &[&str]) {
    let encoded = url_encode_path(cwd);
    let dir = store.join(&encoded).join(id);
    fs::create_dir_all(cwd).unwrap();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("summary.json"), summary.to_string()).unwrap();
    let mut history = String::new();
    for turn in turns {
        history.push_str(
            &json!({
                "type": "user",
                "content": [{"type": "text", "text": format!("<user_query>{turn}</user_query>")}]
            })
            .to_string(),
        );
        history.push('\n');
        history.push_str(&json!({"type": "assistant", "content": "答复"}).to_string());
        history.push('\n');
    }
    fs::write(dir.join("chat_history.jsonl"), history).unwrap();
}

/// CLI 自己的编码：百分号编码，且只按目录名需要。
fn url_encode_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::with_capacity(raw.len() + 8);
    for byte in raw.bytes() {
        match byte {
            b'/' => out.push_str("%2F"),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 记 argv 和 cwd 的假 CLI：`--resume` 与目录只能由它自己来作证。
fn fake_agent(record: &Path) -> PathBuf {
    let script = record.with_extension("sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n{{ echo \"cwd=$PWD\"; for a in \"$@\"; do echo \"arg=$a\"; done; }} >> '{}'\nprintf '%s\\n' '{}'\n",
            record.display(),
            json!({"text": "已经修好了，端口在听。"})
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

/// 一个装着两段对话（两个目录）的镜像源，以及跑着它的主机。
struct Harness {
    /// 主机自己的目录（假 CLI 的默认 cwd），也是两段对话所在临时树的根。
    root: tempfile::TempDir,
    store: PathBuf,
    project_a: PathBuf,
    project_b: PathBuf,
    record: PathBuf,
}

fn harness() -> Harness {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("chaos-store");
    fs::create_dir_all(&store).unwrap();
    let project_a = root.path().join("work").join("web-service");
    let project_b = root.path().join("work").join("backup-audit");
    write_session(
        &store,
        &project_a,
        FIRST,
        json!({
            "info": {"id": FIRST, "cwd": project_a},
            "generated_title": "3081 端口挂了",
            "created_at": "2026-10-08T01:00:00.000000Z",
            "last_active_at": "2026-10-08T02:00:00.000000Z",
            "num_chat_messages": 4
        }),
        &["为什么端口不通"],
    );
    write_session(
        &store,
        &project_b,
        SECOND,
        json!({
            "info": {"id": SECOND, "cwd": project_b},
            "session_summary": "",
            "created_at": "2026-10-09T01:00:00.000000Z",
            "last_active_at": "2026-10-09T03:30:00.000000Z",
            "num_chat_messages": 2
        }),
        &["备份还活着吗"],
    );
    Harness {
        record: root.path().join("agent-record.txt"),
        project_a,
        project_b,
        store,
        root,
    }
}

fn engine(h: &Harness) -> Engine {
    let engine = Engine::with_adapter(HeadlessProcessAdapter::new(
        fake_agent(&h.record),
        h.root.path(),
    ))
    .with_chaos_store_root(&h.store);
    let report = engine.import_chaos_sessions(&h.store);
    assert_eq!(report.scanned, 2, "镜像源里两段对话都要认出来：{report:?}");
    engine
}

async fn serve(engine: Engine) -> String {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(60)).await })
            .await;
    });
    address.to_string()
}

async fn connect(
    address: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .expect("ZCode 端点应当接上")
        .0
}

/// 列表要重试：答复从引擎线程回来，可能慢一拍。
async fn list_until(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    name: &str,
    channel: &str,
    arg: Value,
    want: usize,
) -> Vec<Value> {
    for attempt in 0..60_u32 {
        socket
            .send(Message::Binary(
                call_frame(10 + attempt, channel, name, arg.clone()).into(),
            ))
            .await
            .unwrap();
        let (_, ok, body) = reply(socket).await;
        assert!(ok, "{name} 失败了：{body}");
        let items: Vec<Value> = if let Some(array) = body.as_array() {
            array.clone()
        } else if let Some(array) = body.get("items").and_then(Value::as_array) {
            array.clone()
        } else {
            panic!("{name} 的回包不是列表：{body}");
        };
        if items.len() >= want {
            return items;
        }
    }
    panic!("{name} 始终没列出 {want} 行");
}

#[tokio::test]
async fn the_sidebar_lists_each_conversation_under_its_own_directory_and_time() {
    let h = harness();
    let address = serve(engine(&h)).await;
    let mut socket = connect(&address).await;

    for (channel, name, arg) in [
        ("zcode-task", "listTasks", json!({})),
        (
            "window-controller",
            "listTaskList",
            json!({"kind": "timeline"}),
        ),
    ] {
        let rows = list_until(&mut socket, name, channel, arg, 2).await;
        let a = rows
            .iter()
            .find(|row| row["taskId"] == FIRST || row["sessionId"] == FIRST)
            .or_else(|| {
                rows.iter()
                    .find(|row| row["title"].as_str().is_some_and(|t| t.contains("3081")))
            })
            .unwrap_or_else(|| {
                panic!(
                    "{channel}/{name} 没把 web-service 那段列出来：{}",
                    json!(rows)
                )
            });
        let b = rows
            .iter()
            .find(|row| {
                row["title"]
                    .as_str()
                    .is_some_and(|t| t.contains("备份还活着吗"))
            })
            .unwrap_or_else(|| {
                panic!(
                    "{channel}/{name} 没把 backup-audit 那段列出来：{}",
                    json!(rows)
                )
            });

        // 每一行说自己那个目录。这一条就是这个需求的全部：「哪些目录聊的」要在
        // 界面上看得见，而不是全部记到服务启动目录头上。
        // 比尾段而不是比全路径：主机可能把 temp 目录规范化成另一个写法，而那与
        // 「这一行报的是哪个项目」无关。
        for (row, project) in [(&a, &h.project_a), (&b, &h.project_b)] {
            let reported = row["workspacePath"].as_str().unwrap_or_default();
            assert!(
                std::path::Path::new(reported).ends_with(project),
                "{channel}/{name} 把对话记错了目录：{reported} 应当以 {} 结尾：{row}",
                project.display()
            );
        }

        // 时间是对话自己的。2026-10-08/09 与「刚刚」差得远，一眼可辨。
        let a_updated = a["updatedAt"].as_u64().unwrap_or(0);
        let b_updated = b["updatedAt"].as_u64().unwrap_or(0);
        assert!(a_updated > 0 && b_updated > 0, "{a} / {b}");
        assert_ne!(a_updated, b_updated, "两段的时刻不该被抹平成同一个「刚刚」");
        assert!(
            b_updated > a_updated,
            "backup-audit（10-09）比 web-service（10-08）新：{a} / {b}"
        );
    }
}

#[tokio::test]
async fn continuing_a_terminal_conversation_resumes_it_in_its_own_directory() {
    let h = harness();
    let address = serve(engine(&h)).await;
    let mut socket = connect(&address).await;

    let rows = list_until(&mut socket, "listTasks", "zcode-task", json!({}), 2).await;
    let web = rows
        .iter()
        .find(|row| row["title"].as_str().is_some_and(|t| t.contains("3081")))
        .expect("列表里要有终端里那段 3081 的对话");
    let task_id = web["taskId"].as_str().unwrap().to_owned();

    socket
        .send(Message::Binary(
            call_frame(
                3,
                "zcode-task",
                "sendPrompt",
                json!({"taskId": task_id, "content": "现在端口通了吗"}),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "sendPrompt 失败了：{body}");

    // 假 CLI 记下的东西就是「怎么继续的」唯一可信的凭据。
    let mut record = String::new();
    for _ in 0..200 {
        record = fs::read_to_string(&h.record).unwrap_or_default();
        if record.contains("arg=--resume") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        record.contains(&format!("arg=--resume\narg={FIRST}")),
        "继续对话必须是让终端按它自己的会话 id 恢复。假 CLI 看到的一切：\n{record}"
    );
    let reported_cwd = record
        .lines()
        .find_map(|line| line.strip_prefix("cwd="))
        .unwrap_or_default();
    assert!(
        std::path::Path::new(reported_cwd).ends_with(&h.project_a),
        "问的是 web-service 里的事，就得在 web-service 里答。实际 cwd：{reported_cwd}；假 CLI 看到的一切：\n{record}"
    );
    assert!(
        !record.contains("arg=--resume\narg={SECOND}"),
        "别把别的目录里的会话续上：\n{record}"
    );
}

/// 对着**这台机器真实的** `~/.chaos/sessions` 跑一遍：镜像出来的每一行都必须说得出
/// 自己来自哪个目录，而且那个目录真的存在。默认不跑（`#[ignore]`）——它读的是操作者
/// 自己的会话，不是造出来的 fixture；`cargo test -p xai-grok-web -- --ignored` 手动跑。
#[tokio::test]
#[ignore = "读真实 ~/.chaos/sessions，需要操作者本机环境"]
async fn the_real_store_on_this_machine_is_mirrored_with_its_directories() {
    let Some(store) = chaos_engine::Engine::chaos_store_dir() else {
        eprintln!("这台机器没有会话存储，跳过");
        return;
    };
    let engine = Engine::new().with_chaos_store_root(&store);
    let report = engine.import_chaos_sessions(&store);
    eprintln!("镜像 {store:?}：{report:?}");
    let address = serve(engine).await;
    let mut socket = connect(&address).await;
    let rows = list_until(&mut socket, "listTasks", "zcode-task", json!({}), 1).await;
    for row in &rows {
        let path = row["workspacePath"].as_str().unwrap_or_default();
        assert!(
            !path.is_empty() && std::path::Path::new(path).is_dir(),
            "镜像出来的对话必须落在一个真实目录上：{row}"
        );
        eprintln!(
            "  {} · {} · 更新 {}",
            path,
            row["title"].as_str().unwrap_or_default(),
            row["updatedAt"].as_u64().unwrap_or_default()
        );
    }
    assert!(
        rows.len() >= report.scanned.min(1),
        "扫到 {} 段，列表里只有 {} 行",
        report.scanned,
        rows.len()
    );

    // 打开最长的那一段：系统提示词、注入的 `<system-reminder>`、工具结果都不该
    // 成为界面上的行，而真正说过的话一行都不能少。
    let biggest = rows
        .iter()
        .max_by_key(|row| row["messageCount"].as_u64().unwrap_or(0))
        .expect("列表非空");
    let task_id = biggest["taskId"].as_str().unwrap().to_owned();
    let engine = Engine::new().with_chaos_store_root(&store);
    engine.import_chaos_sessions(&store);
    let id = uuid::Uuid::parse_str(&task_id).unwrap();
    let messages = engine
        .handle(chaos_engine::ClientMessage::Resume {
            client_msg_id: "open".into(),
            session_id: id,
            workspace_id: None,
        })
        .into_iter()
        .find_map(|event| match event {
            chaos_engine::ServerMessage::SessionSnapshot { messages, .. } => Some(messages),
            _ => None,
        })
        .unwrap_or_default();
    eprintln!(
        "打开 {}（{} 条记录）：读到 {} 行",
        biggest["title"].as_str().unwrap_or_default(),
        biggest["messageCount"].as_u64().unwrap_or(0),
        messages.len()
    );
    for message in messages.iter().take(2) {
        eprintln!(
            "  {}: {}",
            message.role,
            message.text.chars().take(60).collect::<String>()
        );
    }
    assert!(
        !messages.is_empty(),
        "真实存储里最长的一段对话读不出任何一行：{biggest}"
    );
    assert!(
        messages
            .iter()
            .all(|m| m.role == "user" || m.role == "assistant"),
        "转写里只该有人和助手的话"
    );
    assert!(
        messages
            .iter()
            .all(|m| !m.text.contains("<system-reminder>") && !m.text.contains("<user_query>")),
        "注入的上下文与包装标签不该出现在界面上"
    );
}
