//! `file.searchWorkspaceFiles` 走一遍真前端会走的形状。
//!
//! 官方前端（`IFileService.searchWorkspaceFiles`）的 @ 面板按每次按键调它，参数是
//! `{rootPath, workspaceIdentity, query, limit}`，期望回有界的 `WorkspaceFileEntry[]`
//! （`name` / `path` / `relativePath` / `type`）；补扫时它带上 `refresh: true`。
//! 这条测试用真 axum 端口、真 WebSocket、官方二进制帧把四件事钉住：主机自己匹配并
//! 回条目、工作区根与其余 `file.*` 受同一条 confinement、缺 `rootPath` 是 promise
//! error 而不是空数组、`limit` 照实照办。
//!
//! 前端本体不在本仓库（它在宿主机器的 `~/.chaos/web/zcode-assets` 里），所以断言的
//! 是它的 sourcemap 里那份 `IFileService` 合同，不是猜的形状。

use std::time::Duration;

use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::{preview::PreviewConfig, router_with_auth_and_previews_and_file_root, web_auth};
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

fn relative_paths(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry["relativePath"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 起一个宿主，文件根由参数给：这样工作区与根外目录都在临时目录里，
/// 不依赖测试进程的工作目录，也不在检出里留垃圾。
async fn host_with_file_root(root: std::path::PathBuf) -> String {
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

#[tokio::test]
async fn search_workspace_files_answers_the_official_entry_shape() {
    let served = tempfile::tempdir().unwrap();
    let workspace = served.path().join("workspace");
    std::fs::create_dir_all(workspace.join("docs")).unwrap();
    std::fs::write(workspace.join("docs/workspace-notes.md"), "notes").unwrap();
    std::fs::write(workspace.join("top-level.md"), "").unwrap();
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;

    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");
    socket
        .send(Message::Binary(
            call_frame(
                1,
                "searchWorkspaceFiles",
                json!({
                    "rootPath": workspace.to_string_lossy(),
                    "query": "workspace-notes",
                    "limit": 100,
                }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 1);
    assert!(ok, "根内的工作区搜索应当成功：{body}");

    let entries = body.as_array().expect("答复必须是数组");
    assert_eq!(entries.len(), 1, "{body}");
    assert_eq!(entries[0]["name"], "workspace-notes.md");
    assert_eq!(entries[0]["relativePath"], "docs/workspace-notes.md");
    assert_eq!(entries[0]["type"], "file");
    assert_eq!(
        entries[0]["path"],
        json!(workspace.join("docs/workspace-notes.md").to_string_lossy())
    );

    // 空 query 是 @ 面板打开时那一次：文件排在目录之前，且都在。
    socket
        .send(Message::Binary(
            call_frame(
                2,
                "searchWorkspaceFiles",
                json!({ "rootPath": workspace.to_string_lossy(), "query": "" }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 2);
    assert!(ok, "{body}");
    let mut listed = relative_paths(&body);
    listed.sort();
    assert_eq!(
        listed,
        vec![
            "docs".to_owned(),
            "docs/workspace-notes.md".to_owned(),
            "top-level.md".to_owned(),
        ],
        "{body}"
    );
    assert_eq!(body[0]["type"], "file", "空 query 先列文件：{body}");
}

#[tokio::test]
async fn search_workspace_files_honors_the_limit_and_refuses_an_outside_root() {
    let served = tempfile::tempdir().unwrap();
    let workspace = served.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    for index in 0..5 {
        std::fs::write(workspace.join(format!("note-{index}.md")), "").unwrap();
    }
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("note-outside.md"), "").unwrap();
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;

    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");
    socket
        .send(Message::Binary(
            call_frame(
                1,
                "searchWorkspaceFiles",
                json!({
                    "rootPath": workspace.to_string_lossy(),
                    "query": "note",
                    "limit": 2,
                }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 1);
    assert!(ok, "{body}");
    assert_eq!(
        body.as_array().map(Vec::len),
        Some(2),
        "limit 是上限，不是建议：{body}"
    );

    // 根外的工作区：拒，并把「哪个根」说清楚，操作者才有的可改。
    socket
        .send(Message::Binary(
            call_frame(
                2,
                "searchWorkspaceFiles",
                json!({
                    "rootPath": outside.path().to_string_lossy(),
                    "query": "note",
                }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 2);
    assert!(!ok, "根外的工作区竟然搜到了：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("CHAOS_WEB_FILE_ROOT")),
        "报错得告诉操作者怎么放开：{body}"
    );

    // 缺 rootPath：有名字的报错，不是静默空数组。
    socket
        .send(Message::Binary(
            call_frame(3, "searchWorkspaceFiles", json!({ "query": "note" })).into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 3);
    assert!(!ok, "缺 rootPath 不该被当成空工作区：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("rootPath")),
        "{body}"
    );
}

#[tokio::test]
async fn search_workspace_files_respects_the_workspace_ignore_rules() {
    let served = tempfile::tempdir().unwrap();
    let workspace = served.path().join("workspace");
    std::fs::create_dir_all(workspace.join("generated")).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join(".gitignore"), "generated/\n").unwrap();
    std::fs::write(workspace.join("generated/note.md"), "").unwrap();
    std::fs::write(workspace.join("note.md"), "").unwrap();
    let endpoint = host_with_file_root(served.path().to_path_buf()).await;

    let (mut socket, _) = connect_async(&endpoint).await.expect("ZCode 端点应当接上");
    socket
        .send(Message::Binary(
            call_frame(
                1,
                "searchWorkspaceFiles",
                json!({
                    "rootPath": workspace.to_string_lossy(),
                    "query": "note",
                    "limit": 100,
                }),
            )
            .into(),
        ))
        .await
        .unwrap();
    let (id, ok, body) = reply(&mut socket).await;
    assert_eq!(id, 1);
    assert!(ok, "{body}");
    // 逐个断言这两条性质，不比对整张名单：糊匹配连绝对路径一起打分（前端的
    // keyword 档），工作区所在的临时目录名也可能凑出一个短 query 的子序列。
    let paths = relative_paths(&body);
    assert!(
        paths.contains(&"note.md".to_owned()),
        "工作区里的 note.md 应当被搜到：{paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.starts_with("generated/")),
        "被 .gitignore 排除的目录不该出现：{paths:?}"
    );
}
