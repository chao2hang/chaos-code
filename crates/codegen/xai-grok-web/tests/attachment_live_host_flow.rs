//! 附件端到端，跑在**构建出的 `chaos-web` 二进制**上，而不是进程内的 router。
//!
//! `attachment_upload_v4.rs` 已经把六个 `attachment*V4` 方法的形状钉在真实
//! axum 端口上；这一条补的是它够不到的一段：`main.rs` 的启动与环境（`CHAOS_WEB_FILE_ROOT`、
//! `CHAOS_AGENT_BINARY`）、以及提交之后那个真实 Agent 进程到底收到了什么。
//!
//! Composer 粘贴一张图时走的是「上行落盘 → `firstInput.attachments` → 发送」，
//! 而图片要让模型看见，最终得靠 Agent 进程自己的提示词里有那张图的路径。这里用
//! 生产同款 `HeadlessProcessAdapter`（`--single <prompt>`，提示词是最后一个参数）+
//! 一个把自己最后那个参数抄下来的假 Agent，断言真宿主确实把 `<image_files>` 块和
//! 落盘路径交给了 Agent 进程——顺带钉住行上的附件引用与正文。

use std::io::BufRead;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

/// 一张真的 1x1 PNG：与 `attachment_upload_v4.rs` 同一份字节。
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("free port")
        .local_addr()
        .unwrap()
        .port()
}

/// 把自己最后那个参数（`--single` 的值，也就是提示词）抄到文件里，再照 headless
/// 约定吐一轮最小输出。抄的那份就是这次断言的对象：模型能看见哪张图，取决于这个
/// 提示词里有没有那张图的路径。
fn write_scripted_agent(dir: &Path) -> PathBuf {
    let script = dir.join("fake-agent.sh");
    std::fs::write(
        &script,
        concat!(
            "#!/usr/bin/env bash\n",
            "printf '%s' \"${@: -1}\" > \"$CHAOS_PROBE_PROMPT_FILE\"\n",
            "printf '%s\\n' '{\"type\":\"text\",\"data\":\"看到了\"}'\n",
            "printf '%s\\n' '{\"type\":\"end\",\"stopReason\":\"end_turn\",\"sessionId\":\"s\",\"requestId\":\"r\"}'\n",
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

struct Host {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
    port: u16,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Host {
    /// 等启动那一行。等不到就把宿主自己的 stderr 摊开——启动期的拒绝都写在那里。
    async fn wait_for(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let logged = self.lines.lock().unwrap().join("\n");
            if logged.contains(needle) {
                return logged;
            }
            if let Some(status) = self.child.try_wait().expect("poll host") {
                panic!("宿主在报出 {needle:?} 之前就退出了（{status}）：\n{logged}");
            }
            assert!(Instant::now() < deadline, "等不到 {needle:?}：\n{logged}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// 起真二进制：环境按 `host_startup.rs` 的口径清干净，只留这一条测试要的那几项。
fn start_host(env: &[(&str, &str)]) -> Host {
    let port = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_chaos-web"));
    command
        .env("CHAOS_WEB_PORT", port.to_string())
        .env("CHAOS_WEB_AUTH", "0")
        .env("CHAOS_DISABLE_CONFIG_FALLBACK", "1")
        .env_remove("CHAOS_WEB_STATE")
        .env_remove("CHAOS_WORKSPACE_ROOT")
        .env_remove("CHAOS_WEB_SQLITE")
        .env_remove("CHAOS_AGENT_BINARY")
        .env_remove("CHAOS_AGENT_CWD")
        .env_remove("CHAOS_PROVIDER_BASE_URL")
        .env_remove("CHAOS_PROVIDER_MODEL")
        .env_remove("CHAOS_PROVIDER_API_KEY")
        .env_remove("CHAOS_SAFE_WEB_MODE")
        .env_remove("CHAOS_WEB_TOKEN")
        .env_remove("CHAOS_WEB_AUTH_FILE")
        .env_remove("CHAOS_WEB_BIND")
        .env_remove("CHAOS_WEB_PUBLIC_ORIGIN")
        .env_remove("CHAOS_WEB_ASSETS_DIR")
        .env_remove("CHAOS_WEB_DEV_ORIGIN")
        .env_remove("CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN")
        .env_remove("CHAOS_WEB_FILE_ROOT")
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // Test fixture. `Host::drop` kills and reaps it; enrolling it would put the
    // test harness's own process group into the kill set.
    #[allow(clippy::disallowed_methods)]
    let mut child = command.spawn().expect("start chaos-web");
    let stderr = child.stderr.take().expect("piped stderr");
    let lines = Arc::new(Mutex::new(Vec::new()));
    {
        let lines = Arc::clone(&lines);
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                lines.lock().unwrap().push(line);
            }
        });
    }
    Host { child, lines, port }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn inner(bytes: &[u8]) -> &[u8] {
    let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    &bytes[13..13 + len]
}

async fn send_call(socket: &mut Socket, id: u32, name: &str, arg: Value) {
    let frame = write_frame(&write_message(
        &RpcValue::Json(json!([rpc_type::PROMISE, id, "zcode-agent", name])),
        &RpcValue::Json(arg),
    ));
    socket.send(Message::Binary(frame.into())).await.unwrap();
}

/// 取回下一次调用的答复（成功给 body，失败给错误对象），跳过服务端自己的帧。
async fn reply(socket: &mut Socket) -> (u64, bool, Value) {
    loop {
        let frame = match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => bytes,
            other => panic!("应当收到二进制帧，实际 {other:?}"),
        };
        if frame.len() < 13 {
            continue;
        }
        let Ok((header, body)) = read_message(inner(&frame)) else {
            continue;
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        let kind = items.first().and_then(Value::as_u64).unwrap_or_default();
        if kind != rpc_type::PROMISE_SUCCESS as u64 && kind != rpc_type::PROMISE_ERROR as u64 {
            continue;
        }
        let id = items.get(1).and_then(Value::as_u64).unwrap_or_default();
        let body = match body {
            RpcValue::Json(value) => value,
            RpcValue::Undefined => Value::Null,
            RpcValue::Binary(bytes, _) => json!({ "binary": bytes }),
        };
        return (id, kind == rpc_type::PROMISE_SUCCESS as u64, body);
    }
}

/// 官方 Composer 的上行：begin → 逐片 chunk → commit。返回 `ref`。
async fn upload(
    socket: &mut Socket,
    id: &mut u32,
    workspace: &Path,
    session: &str,
    bytes: &[u8],
) -> String {
    send_call(
        socket,
        *id,
        "attachmentBeginV4",
        json!({
            "connectionId": "conn-1",
            "uploadId": "upload-live-1",
            "sessionId": session,
            "workspacePath": workspace.to_string_lossy(),
            "fileName": "screenshot.png",
            "mime": "image/png",
            "totalBytes": bytes.len(),
            "totalChunks": 2,
            "checksum": format!("sha256:{}", sha256_hex(bytes)),
        }),
    )
    .await;
    let (_, ok, body) = reply(socket).await;
    assert!(ok, "begin 应当成功：{body}");
    assert_eq!(body["state"], "staging", "{body}");

    // 两片：第二片故意只放几个字节，走的是真实的分片边界。
    let split = bytes.len() - 3;
    for (index, chunk) in [&bytes[..split], &bytes[split..]].iter().enumerate() {
        *id += 1;
        send_call(
            socket,
            *id,
            "attachmentChunkV4",
            json!({
                "uploadId": "upload-live-1",
                "sessionId": session,
                "chunkIndex": index,
                "dataBase64": encode(chunk),
            }),
        )
        .await;
        let (_, ok, body) = reply(socket).await;
        assert!(ok, "第 {index} 片应当收下：{body}");
        // 前端的进度校验钉在这条等式上：nextChunkIndex === chunkIndex + 1。
        assert_eq!(body["nextChunkIndex"], index as u64 + 1, "{body}");
    }

    *id += 1;
    send_call(
        socket,
        *id,
        "attachmentCommitV4",
        json!({ "uploadId": "upload-live-1", "sessionId": session }),
    )
    .await;
    let (_, ok, body) = reply(socket).await;
    assert!(ok, "commit 应当成功：{body}");
    body["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("commit 必回 ref：{body}"))
        .to_owned()
}

/// 读下一个会话主题帧（EventFire 的 wire 载荷），静默 10s 视作帧流结束。
async fn next_conversation_frame(socket: &mut Socket) -> Option<Value> {
    loop {
        let message = match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(message))) => message,
            _ => return None,
        };
        let Message::Binary(bytes) = message else {
            continue;
        };
        if bytes.len() < 13 {
            continue;
        }
        let Ok((header, body)) = read_message(inner(&bytes)) else {
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
        if value["frame"]["topic"]
            .as_str()
            .is_some_and(|topic| topic.starts_with("conversation/"))
        {
            return Some(value);
        }
    }
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
    if let Some(deltas) = payload["deltas"].as_array() {
        if let Some(row) = deltas
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

#[tokio::test]
async fn a_composer_image_reaches_the_real_agent_prompt_through_the_built_host() {
    let root = tempfile::tempdir().expect("文件根");
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = tempfile::tempdir().expect("临时 HOME");
    let agent_dir = tempfile::tempdir().expect("临时 Agent 目录");
    let prompt_file = agent_dir.path().join("prompt.txt");
    let agent = write_scripted_agent(agent_dir.path());

    let mut host = start_host(&[
        ("CHAOS_WEB_FILE_ROOT", root.path().to_str().unwrap()),
        ("CHAOS_AGENT_BINARY", agent.to_str().unwrap()),
        ("CHAOS_AGENT_CWD", workspace.to_str().unwrap()),
        ("CHAOS_PROBE_PROMPT_FILE", prompt_file.to_str().unwrap()),
        ("HOME", home.path().to_str().unwrap()),
    ]);
    host.wait_for("Chaos Web listening on").await;

    let endpoint = format!("ws://127.0.0.1:{}/ws/zcode", host.port);
    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");

    // 上行时的 sessionId 只是落盘目录名；真正的会话 id 由 createSession 给出。
    let upload_session = "6b7f1c93-2d44-4a0e-8b31-0d5f7c9e2a10";
    let mut rpc_id = 1;
    let reference = upload(&mut socket, &mut rpc_id, &workspace, upload_session, PNG).await;

    // 落盘路径就是 `ref`：提示词里的 `<image_files>` 与 Agent 自己的 read_file
    // 指的是同一个文件。
    let expected = workspace
        .join(".chaos/uploads")
        .join(upload_session)
        .join("upload-live-1")
        .join("screenshot.png");
    assert_eq!(Path::new(&reference), expected);
    assert_eq!(std::fs::read(&expected).unwrap(), PNG, "落盘的必须是原件");

    // 官方启动序：会话主题的监听先于 createSession。
    socket
        .send(Message::Binary(
            write_frame(&write_message(
                &RpcValue::Json(json!([
                    rpc_type::EVENT_LISTEN,
                    500,
                    "zcode-agent",
                    "onDynamicConversationFrame"
                ])),
                &RpcValue::Undefined,
            ))
            .into(),
        ))
        .await
        .unwrap();

    rpc_id += 1;
    send_call(
        &mut socket,
        rpc_id,
        "sendConversationCommandV4",
        json!({
            "workspacePath": workspace.to_string_lossy(),
            "envelope": {
                "commandId": "cmd-live-create",
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
    assert_eq!(id, u64::from(rpc_id));
    assert!(ok, "createSession 应当被接受：{body}");
    let session_id = body["result"]["sessionId"]
        .as_str()
        .expect("createSession 结果要带 sessionId")
        .to_owned();

    // 真 Agent 进程收到的那份提示词：图片路径在这里，模型才可能读到它。
    let deadline = Instant::now() + Duration::from_secs(30);
    let prompt = loop {
        if let Ok(prompt) = std::fs::read_to_string(&prompt_file) {
            if !prompt.is_empty() {
                break prompt;
            }
        }
        assert!(
            Instant::now() < deadline,
            "等不到真 Agent 进程被拉起（提示词文件 {} 一直没出现）",
            prompt_file.display()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        prompt.contains("<image_files>"),
        "提示词缺图片信封：\n{prompt}"
    );
    assert!(
        prompt.contains(&reference),
        "信封里得有落盘路径 {reference}：\n{prompt}"
    );
    assert!(
        prompt.contains("这张图里是什么？"),
        "用户敲的那句必须在信封之后照旧送到：\n{prompt}"
    );

    // 行上的附件是前端画缩略图的唯一依据。
    rpc_id += 1;
    send_call(
        &mut socket,
        rpc_id,
        "subscribeConversationV4",
        json!({ "sessionId": session_id }),
    )
    .await;
    let (_, ok, body) = reply(&mut socket).await;
    assert!(ok, "{body}");

    let row = loop {
        let Some(frame) = next_conversation_frame(&mut socket).await else {
            panic!("等不到带附件的 userInput 行");
        };
        let payload = &frame["frame"]["payload"];
        if let Some(row) = find_user_row(payload) {
            break row;
        }
        // 锚点快照可能落在 createSession 的行落地之前：稍等再要一次。
        if payload["kind"] == "snapshot" {
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    };
    // 用户敲的那句原样在行上：附件信封只进提示词，不进正文。
    assert_eq!(row["text"], "这张图里是什么？", "{row}");
    assert_eq!(row["attachments"][0]["ref"], json!(reference), "{row}");
    assert_eq!(row["attachments"][0]["mime"], "image/png", "{row}");
    assert_eq!(row["attachments"][0]["bytes"], json!(PNG.len()), "{row}");
}
