//! `zcode-agent` 附件通道（`attachment*V4`）走一遍官方 Composer 会走的形状。
//!
//! 官方壳粘贴/选择一张图时，Composer 从不整份发送：`attachmentBeginV4` 开一个暂存
//! 条目，`attachmentChunkV4` 按 384 KiB 追加 `dataBase64`，`attachmentCommitV4` 核
//! 声明的 `sha256:` 摘要之后才认那个 `ref`；随后对话行按 `attachmentPreviewSourceV4`
//! + `attachmentReadV4` 把字节分块读回去画缩略图。这六个方法此前一个都没有，每一次粘贴
//! 都停在 `Chaos 后端未实现通道方法 zcode-agent.attachmentBeginV4`——附件永远进不了
//! ready 态，发送按钮也就一直不亮。
//!
//! 这条测试用真 axum 端口、真 WebSocket、官方二进制帧把三件事钉住：字节真的按分片落盘
//! 且能按 512 KiB 分块读回、落盘被 `CHAOS_WEB_FILE_ROOT` 约束（并如实拒绝根外路径）、
//! 除这六个方法之外的 `zcode-agent` 方法照旧是有名字的「未实现」，不会被这个通道整片
//! 吞掉。
//!
//! 前端本体不在本仓库（它在宿主机器的 `~/.chaos/web/zcode-assets` 里），所以断言的
//! 是它 sourcemap 里那份 `IAttachmentService` 合同，不是猜的形状。

use std::time::Duration;

use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::{preview::PreviewConfig, router_with_auth_and_previews_and_file_root, web_auth};
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

/// 一张真的 1x1 PNG：签名、切片与摘要都照生产那份走。
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xfe, 0xd4, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode(text: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("dataBase64 必须是合法 base64")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

fn call_frame(channel: &str, id: u32, name: &str, arg: Value) -> Vec<u8> {
    write_frame(&write_message(
        &RpcValue::Json(json!([rpc_type::PROMISE, id, channel, name])),
        &RpcValue::Json(arg),
    ))
}

fn listen_frame(channel: &str, id: u32, name: &str) -> Vec<u8> {
    write_frame(&write_message(
        &RpcValue::Json(json!([rpc_type::EVENT_LISTEN, id, channel, name])),
        &RpcValue::Undefined,
    ))
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn send_call(socket: &mut Socket, channel: &str, id: u32, name: &str, arg: Value) {
    socket
        .send(Message::Binary(call_frame(channel, id, name, arg).into()))
        .await
        .unwrap();
}

/// 取回下一次调用的答复：成功给 body，失败给错误对象。
async fn reply(socket: &mut Socket) -> (u64, bool, Value) {
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

/// 起一个宿主，文件根由参数给：这样工作区与根外目录都在临时目录里，
/// 不依赖测试进程的工作目录，也不在检出里留垃圾。
async fn host_with_file_root(root: std::path::PathBuf) -> String {
    isolate_settings_home();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let router = router_with_auth_and_previews_and_file_root(
        Engine::new(),
        "",
        false,
        None,
        PreviewConfig::disabled(),
        web_auth::WebAuth::disabled(),
        Some(root),
    );
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(30)).await })
            .await;
    });
    format!("ws://{address}/ws/zcode")
}

/// Composer 的一次完整上行：begin → 逐片 chunk → commit。返回 `ref`。
async fn upload(
    socket: &mut Socket,
    rpc_id: &mut u32,
    workspace: &std::path::Path,
    session_id: &str,
    upload_id: &str,
    bytes: &[u8],
    chunks: &[&[u8]],
) -> String {
    let id = *rpc_id;
    *rpc_id += 1;
    send_call(
        socket,
        "zcode-agent",
        id,
        "attachmentBeginV4",
        json!({
            "connectionId": "conn-1",
            "uploadId": upload_id,
            "sessionId": session_id,
            "workspacePath": workspace.to_string_lossy(),
            "fileName": "screenshot.png",
            "mime": "image/png",
            "totalBytes": bytes.len(),
            "totalChunks": chunks.len(),
            "checksum": format!("sha256:{}", sha256_hex(bytes)),
        }),
    )
    .await;
    let (id, ok, body) = reply(socket).await;
    assert_eq!(id, u64::from(*rpc_id - 1));
    assert!(ok, "begin 应当成功：{body}");
    assert_eq!(body["state"], "staging", "{body}");
    assert_eq!(body["nextChunkIndex"], 0, "{body}");

    for (index, chunk) in chunks.iter().enumerate() {
        let id = *rpc_id;
        *rpc_id += 1;
        send_call(
            socket,
            "zcode-agent",
            id,
            "attachmentChunkV4",
            json!({
                "uploadId": upload_id,
                "sessionId": session_id,
                "chunkIndex": index,
                "dataBase64": encode(chunk),
            }),
        )
        .await;
        let (id, ok, body) = reply(socket).await;
        assert_eq!(id, u64::from(*rpc_id - 1));
        assert!(ok, "第 {index} 片应当收下：{body}");
        // 前端的进度校验钉在这条等式上：nextChunkIndex === chunkIndex + 1。
        assert_eq!(body["nextChunkIndex"], index as u64 + 1, "{body}");
    }

    let id = *rpc_id;
    *rpc_id += 1;
    send_call(
        socket,
        "zcode-agent",
        id,
        "attachmentCommitV4",
        json!({ "uploadId": upload_id, "sessionId": session_id }),
    )
    .await;
    let (id, ok, body) = reply(socket).await;
    assert_eq!(id, u64::from(*rpc_id - 1));
    assert!(ok, "commit 应当成功：{body}");
    body["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("commit 必回 ref：{body}"))
        .to_owned()
}

#[tokio::test]
async fn a_pasted_image_lands_on_disk_and_reads_back_in_slices() {
    let served = tempfile::tempdir().unwrap();
    let workspace = served.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = "9c1f4c1a-6d0e-4a44-9c36-1c9d0a2f5b77";
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;
    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");
    let mut rpc_id = 1;

    // 两片：第二片故意只放几个字节，走的是真实的分片边界。
    let split = 40usize;
    let reference = upload(
        &mut socket,
        &mut rpc_id,
        &workspace,
        session_id,
        "upload-3f2a",
        PNG,
        &[&PNG[..split], &PNG[split..]],
    )
    .await;

    // `ref` 就是盘上那个绝对路径：提示词侧的 `<image_files>` 块与 Agent 自己的
    // read_file 指的是同一个文件。
    let expected = workspace
        .join(".chaos/uploads")
        .join(session_id)
        .join("upload-3f2a")
        .join("screenshot.png");
    assert_eq!(std::path::Path::new(&reference), expected);
    assert_eq!(std::fs::read(&expected).unwrap(), PNG, "落盘的必须是原件");

    // 预览源如实回 chunked：Web 端没有本地媒体协议 URL，用户走的就是分块读。
    let id = rpc_id;
    rpc_id += 1;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "attachmentPreviewSourceV4",
        json!({ "sessionId": session_id, "ref": reference, "clientMode": "web" }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "{body}");
    assert_eq!(body, json!({ "kind": "chunked" }), "{body}");

    // 缩略图取字节：第一片带 nextOffset，尾片 nextOffset 为 null。
    let id = rpc_id;
    rpc_id += 1;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "attachmentReadV4",
        json!({ "sessionId": session_id, "ref": reference, "offset": 0, "limit": 16 }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "{body}");
    assert_eq!(body["mediaType"], "image/png", "{body}");
    assert_eq!(body["totalBytes"], PNG.len(), "{body}");
    assert_eq!(body["nextOffset"], 16, "{body}");
    assert_eq!(decode(body["dataBase64"].as_str().unwrap()), PNG[..16]);

    let id = rpc_id;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "attachmentReadV4",
        json!({
            "sessionId": session_id,
            "ref": reference,
            "offset": 16,
            "limit": 512 * 1024,
        }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "{body}");
    assert!(
        body["nextOffset"].is_null(),
        "尾片的 nextOffset 必须是 null：{body}"
    );
    assert_eq!(decode(body["dataBase64"].as_str().unwrap()), PNG[16..]);

    // 落盘是一次改名收尾的：暂存用的半份临时文件不许留下。
    let parent = expected.parent().unwrap();
    assert!(
        !std::fs::read_dir(parent).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part")),
        "改名之后的临时文件不该留在 {}",
        parent.display()
    );
}

#[tokio::test]
async fn a_tampered_upload_is_refused_by_name_and_leaves_no_file() {
    let served = tempfile::tempdir().unwrap();
    let workspace = served.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = "3ac6b6d2-0c5c-4d2c-8f5c-2c9f4a1b7e10";
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;
    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");
    let mut rpc_id = 1;

    // begin 声明 PNG 的摘要，发的却是同长度的另一份字节：commit 必须在摘要这一步失败。
    let mut tampered = PNG.to_vec();
    tampered[0] ^= 0xff;
    let id = rpc_id;
    rpc_id += 1;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "attachmentBeginV4",
        json!({
            "uploadId": "upload-tampered",
            "sessionId": session_id,
            "workspacePath": workspace.to_string_lossy(),
            "fileName": "screenshot.png",
            "mime": "image/png",
            "totalBytes": PNG.len(),
            "totalChunks": 1,
            "checksum": format!("sha256:{}", sha256_hex(PNG)),
        }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "{body}");

    let id = rpc_id;
    rpc_id += 1;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "attachmentChunkV4",
        json!({
            "uploadId": "upload-tampered",
            "sessionId": session_id,
            "chunkIndex": 0,
            "dataBase64": encode(&tampered),
        }),
    )
    .await;
    let (_, ok, _) = reply(&mut socket).await;
    assert!(ok);

    let id = rpc_id;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "attachmentCommitV4",
        json!({ "uploadId": "upload-tampered", "sessionId": session_id }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(!ok, "摘要不符却提交成功了：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("摘要")),
        "报错得说清是哪一步不过：{body}"
    );
    assert!(
        !workspace.join(".chaos/uploads").join(session_id).exists(),
        "失败的上行不许留下半份附件"
    );
}

#[tokio::test]
async fn a_workspace_outside_the_served_root_is_refused_and_the_channel_stays_scoped() {
    let served = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("shot.png"), PNG).unwrap();
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;
    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");

    // 根外的工作区：与 file.* 同一条 confinement，拒的时候把「哪个根」说清楚。
    send_call(
        &mut socket,
        "zcode-agent",
        1,
        "attachmentBeginV4",
        json!({
            "uploadId": "upload-outside",
            "sessionId": "session-1",
            "workspacePath": outside.path().to_string_lossy(),
            "fileName": "shot.png",
            "mime": "image/png",
            "totalBytes": PNG.len(),
            "totalChunks": 1,
            "checksum": format!("sha256:{}", sha256_hex(PNG)),
        }),
    )
    .await;
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 1);
    assert!(!ok, "根外的工作区竟然开了暂存：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("CHAOS_WEB_FILE_ROOT")),
        "报错得告诉操作者怎么放开：{body}"
    );

    // 这个通道只认那六个方法：别的 zcode-agent 方法照旧是有名字的「未实现」，
    // 不是被整片吞掉的静默。
    send_call(
        &mut socket,
        "zcode-agent",
        2,
        "attachmentPutV4",
        json!({ "uploadId": "x" }),
    )
    .await;
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 2);
    assert!(!ok, "没实现的方法不该答成功：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("attachmentPutV4")),
        "{body}"
    );
}

#[tokio::test]
async fn a_sent_image_rides_both_the_row_and_the_model_prompt() {
    let served = tempfile::tempdir().unwrap();
    let workspace = served.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    // 上行时的 sessionId 只是落盘目录名；真正的会话 id 由 createSession 给出。
    let upload_session = "5f0d2b7e-3a41-4f2b-9d5e-77c1b8a3e642";
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;
    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");
    let mut rpc_id = 1;

    let reference = upload(
        &mut socket,
        &mut rpc_id,
        &workspace,
        upload_session,
        "upload-sent",
        PNG,
        &[PNG],
    )
    .await;

    // 会话主题：官方壳在 createSession 之前就先听这个通道。
    socket
        .send(Message::Binary(
            listen_frame("zcode-agent", 500, "onDynamicConversationFrame").into(),
        ))
        .await
        .unwrap();

    let id = rpc_id;
    rpc_id += 1;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "sendConversationCommandV4",
        json!({
            "workspacePath": workspace.to_string_lossy(),
            "envelope": {
                "commandId": "cmd-create",
                "type": "createSession",
                "payload": {
                    "firstInput": {
                        "text": "这张图里是什么？",
                        "attachments": [{
                            "ref": reference,
                            "fileName": "screenshot.png",
                            "mime": "image/png",
                            "bytes": PNG.len(),
                        }],
                    },
                },
            },
        }),
    )
    .await;
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, u64::from(rpc_id - 1));
    assert!(ok, "createSession 应当被接受：{body}");
    let session_id = body["result"]["sessionId"]
        .as_str()
        .expect("createSession 结果要带 sessionId")
        .to_owned();

    // 订阅拿锚点快照：里面既有引擎的行，也有桥自己刚落的 userInput 行。
    let id = rpc_id;
    send_call(
        &mut socket,
        "zcode-agent",
        id,
        "subscribeConversationV4",
        json!({ "sessionId": session_id }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "{body}");

    let row = next_user_input_row(&mut socket).await;
    // 用户敲的那句原样在行上：附件信封只进提示词，不进正文。
    assert_eq!(row["text"], "这张图里是什么？", "{row}");
    // 行上的附件是前端画缩略图的唯一依据。
    assert_eq!(row["attachments"][0]["ref"], json!(reference), "{row}");
    assert_eq!(row["attachments"][0]["mime"], "image/png", "{row}");
    assert_eq!(row["attachments"][0]["bytes"], json!(PNG.len()), "{row}");
}

/// 收会话帧，直到出现带附件的 userInput 行（锚点快照里，或随后的增量帧里）。
async fn next_user_input_row(socket: &mut Socket) -> Value {
    for _ in 0..40 {
        let Some(frame) = next_frame(socket).await else {
            break;
        };
        let payload = &frame["frame"]["payload"];
        if let Some(row) = find_user_row(payload) {
            return row;
        }
        // 锚点快照可能落在 createSession 的行落地之前：稍等再要一次。
        if payload["kind"] == "snapshot" {
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    }
    panic!("等不到带附件的 userInput 行");
}

fn find_user_row(payload: &Value) -> Option<Value> {
    // 锚点帧把整份会话快照放在 payload.snapshot 里；增量帧把 ops 放在 payload.deltas 里。
    if let Some(rows) = payload["snapshot"]["rows"]["window"].as_array() {
        if let Some(row) = rows
            .iter()
            .find(|row| row["kind"] == "userInput" && row["attachments"].is_array())
        {
            return Some(row.clone());
        }
    }
    if let Some(ops) = payload["deltas"].as_array() {
        if let Some(row) = ops
            .iter()
            .filter(|op| op["op"] == "row.appended")
            .map(|op| op["row"].clone())
            .find(|row| row["kind"] == "userInput" && row["attachments"].is_array())
        {
            return Some(row);
        }
    }
    None
}

/// 读下一个会话主题帧（EventFire 上的 wire 载荷），静默 10s 视作帧流结束。
async fn next_frame(socket: &mut Socket) -> Option<Value> {
    loop {
        let message = match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(message))) => message,
            // 静默 10s 或对端断开都算帧流结束：让调用方把「没来」写进自己的失败信息。
            _ => return None,
        };
        let Message::Binary(bytes) = message else {
            continue;
        };
        if bytes.len() < 13 {
            continue;
        }
        let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
        let Ok((header, body)) = read_message(&bytes[13..13 + len]) else {
            continue;
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
        // 同一条 socket 上还有别的主题在推帧，只认会话主题。
        if value["frame"]["topic"]
            .as_str()
            .is_some_and(|topic| topic.starts_with("conversation/"))
        {
            return Some(value);
        }
    }
}
