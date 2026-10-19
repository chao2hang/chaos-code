//! End-to-end browser password setup, login, and route protection.

use std::{net::SocketAddr, time::Duration};

use axum::Router;
use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async, tungstenite::Message, tungstenite::client::IntoClientRequest,
};
use xai_grok_web::{
    preview::PreviewConfig, router_with_auth_and_previews,
    router_with_auth_and_previews_and_file_root, web_auth::WebAuth,
};

async fn start(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}"), server)
}

fn cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn first_run_setup_login_logout_and_files_are_password_gated() {
    let temp = tempdir().unwrap();
    let assets = temp.path().join("assets");
    std::fs::create_dir_all(assets.join("assets")).unwrap();
    std::fs::write(assets.join("index.html"), "official zcode shell").unwrap();
    std::fs::write(assets.join("assets/app.js"), "shell-bundle").unwrap();
    let auth_path = temp.path().join("auth/web_auth.json");
    let auth = WebAuth::open(&auth_path).unwrap();
    let app = router_with_auth_and_previews_and_file_root(
        Engine::new(),
        "",
        false,
        Some(assets),
        PreviewConfig::disabled(),
        auth,
        Some(std::path::PathBuf::from("/home/chaos")),
    );
    let (origin, server) = start(app).await;
    let client = reqwest::Client::new();

    let first_page = client.get(&origin).send().await.unwrap();
    assert_eq!(first_page.status(), reqwest::StatusCode::OK);
    assert!(first_page.text().await.unwrap().contains("首次设置密钥"));

    let initial_status: Value = client
        .get(format!("{origin}/api/auth/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(initial_status["setup_required"], true);
    assert_eq!(initial_status["authenticated"], false);

    let denied = client
        .get(format!("{origin}/api/server-info"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::UNAUTHORIZED);
    let denied_file = client
        .get(format!("{origin}/assets/app.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_file.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(
        !denied_file.text().await.unwrap().contains("shell-bundle"),
        "an unauthenticated asset request must not receive the login HTML as JavaScript"
    );
    let denied_socket = client
        .get(format!("{origin}/ws/zcode"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_socket.status(), reqwest::StatusCode::UNAUTHORIZED);

    let setup_code_path = temp.path().join("auth/web_auth.json.setup-code");
    let setup_code = std::fs::read_to_string(&setup_code_path).unwrap();
    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({
            "password": "local-first-password-2026",
            "confirmation": "local-first-password-2026",
            "setup_code": setup_code
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);
    let session_cookie = cookie(&setup);
    let set_cookie = setup.headers()[reqwest::header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(set_cookie.contains("httponly"));
    assert!(set_cookie.contains("samesite=strict"));
    assert!(!set_cookie.contains("local-first-password"));

    let info = client
        .get(format!("{origin}/api/server-info"))
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(info.status(), reqwest::StatusCode::OK);
    let server_info = info.json::<Value>().await.unwrap();
    assert_eq!(server_info["authRequired"], true);
    assert!(
        server_info["workspaces"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["path"] == "/home/chaos")),
        "the selected workspace should be exposed from the home-scoped service: {server_info}"
    );
    let mut ws_request = format!("ws://{}/ws/zcode", origin.trim_start_matches("http://"))
        .into_client_request()
        .unwrap();
    ws_request.headers_mut().insert(
        reqwest::header::COOKIE,
        reqwest::header::HeaderValue::from_str(&session_cookie).unwrap(),
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_request)
        .await
        .expect("authenticated browser cookie must open the ZCode RPC socket");
    ws.close(None).await.unwrap();
    assert!(
        !setup_code_path.exists(),
        "one-time code is removed after first password setup"
    );

    let shell = client
        .get(&origin)
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(shell.status(), reqwest::StatusCode::OK);
    assert_eq!(shell.text().await.unwrap(), "official zcode shell");

    let mut zcode_request = format!("ws://{}/ws/zcode", origin.trim_start_matches("http://"))
        .into_client_request()
        .unwrap();
    zcode_request.headers_mut().insert(
        reqwest::header::COOKIE,
        reqwest::header::HeaderValue::from_str(&session_cookie).unwrap(),
    );
    let (mut zcode_socket, _) = tokio_tungstenite::connect_async(zcode_request)
        .await
        .expect("authenticated ZCode socket");
    let mut got_file_root = false;
    for _ in 0..10 {
        let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) =
            zcode_socket.next().await
        else {
            continue;
        };
        if bytes.len() < 13 {
            continue;
        }
        let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
        let Ok((header, body)) = zcode_rpc_protocol::read_message(&bytes[13..13 + len]) else {
            continue;
        };
        let zcode_rpc_protocol::RpcValue::Json(Value::Array(header)) = header else {
            continue;
        };
        if header.first().and_then(Value::as_u64)
            != Some(zcode_rpc_protocol::rpc_type::INITIALIZE as u64)
        {
            continue;
        }
        let _ = body;
        let request = zcode_rpc_protocol::write_message(
            &zcode_rpc_protocol::RpcValue::Json(json!([
                zcode_rpc_protocol::rpc_type::PROMISE,
                43_u32,
                "file",
                "ensureConversationWorkspace"
            ])),
            &zcode_rpc_protocol::RpcValue::Undefined,
        );
        zcode_socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                zcode_rpc_protocol::write_frame(&request).into(),
            ))
            .await
            .unwrap();
        break;
    }
    for _ in 0..20 {
        let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) =
            zcode_socket.next().await
        else {
            continue;
        };
        if bytes.len() < 13 {
            continue;
        }
        let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
        let Ok((header, body)) = zcode_rpc_protocol::read_message(&bytes[13..13 + len]) else {
            continue;
        };
        let zcode_rpc_protocol::RpcValue::Json(Value::Array(header)) = header else {
            continue;
        };
        if header.get(1).and_then(Value::as_u64) != Some(43) {
            continue;
        }
        let zcode_rpc_protocol::RpcValue::Json(workspace) = body else {
            panic!("workspace result")
        };
        assert!(
            workspace["path"]
                .as_str()
                .unwrap()
                .ends_with("workspace/default")
        );
        got_file_root = true;
        break;
    }
    assert!(
        got_file_root,
        "ZCode must discover files under the configured root"
    );
    zcode_socket.close(None).await.unwrap();

    let bundle = client
        .get(format!("{origin}/assets/app.js"))
        .header(reqwest::header::ACCEPT, "application/javascript")
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(bundle.text().await.unwrap(), "shell-bundle");

    let logout = client
        .post(format!("{origin}/api/auth/logout"))
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), reqwest::StatusCode::OK);
    assert!(cookie(&logout).ends_with('='));
    let expired = client
        .get(format!("{origin}/api/server-info"))
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(expired.status(), reqwest::StatusCode::UNAUTHORIZED);

    let login = client
        .post(format!("{origin}/api/auth/login"))
        .json(&json!({ "password": "local-first-password-2026" }))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), reqwest::StatusCode::OK);
    let login_cookie = cookie(&login);
    let restored = client
        .get(format!("{origin}/api/server-info"))
        .header(reqwest::header::COOKIE, login_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(restored.status(), reqwest::StatusCode::OK);

    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}

#[tokio::test]
async fn wrong_password_is_rejected_and_setup_cannot_be_reclaimed() {
    let temp = tempdir().unwrap();
    let auth = WebAuth::open(temp.path().join("web_auth.json")).unwrap();
    let app = router_with_auth_and_previews(
        Engine::new(),
        "",
        false,
        None,
        PreviewConfig::disabled(),
        auth,
    );
    let (origin, server) = start(app).await;
    let client = reqwest::Client::new();
    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({
            "password": "first-password-is-long-enough",
            "confirmation": "first-password-is-long-enough"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);

    let reclaimed = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({
            "password": "attacker-password-long-enough",
            "confirmation": "attacker-password-long-enough"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reclaimed.status(), reqwest::StatusCode::CONFLICT);

    let wrong = client
        .post(format!("{origin}/api/auth/login"))
        .json(&json!({ "password": "wrong-password-long-enough" }))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), reqwest::StatusCode::UNAUTHORIZED);

    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}

#[tokio::test]
async fn authenticated_file_channel_can_readdir_the_real_project_directories() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

    fn rpc_call(id: u32, name: &str, path: &str) -> Vec<u8> {
        write_frame(&write_message(
            &RpcValue::Json(json!([rpc_type::PROMISE, id, "file", name])),
            &RpcValue::Json(json!({"path": path})),
        ))
    }

    let temp = tempdir().unwrap();
    let auth_path = temp.path().join("auth/web_auth.json");
    let auth = WebAuth::open(&auth_path).unwrap();
    let app = router_with_auth_and_previews_and_file_root(
        Engine::new(),
        "",
        false,
        None,
        PreviewConfig::disabled(),
        auth,
        Some(std::path::PathBuf::from("/home/chaos")),
    );
    let (origin, server) = start(app).await;
    let client = reqwest::Client::new();
    let setup_code =
        std::fs::read_to_string(temp.path().join("auth/web_auth.json.setup-code")).unwrap();
    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({
            "password": "workspace-probe-password",
            "confirmation": "workspace-probe-password",
            "setup_code": setup_code
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);
    let session_cookie = cookie(&setup);

    let request = format!("ws://{}/ws/zcode", origin.strip_prefix("http://").unwrap());
    let mut request = request.into_client_request().unwrap();
    request.headers_mut().insert(
        reqwest::header::COOKIE,
        reqwest::header::HeaderValue::from_str(&session_cookie).unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let mut id = 10_u32;
    for directory in ["/home/chaos/hunt/dev/projects/chaos-code", "/home/chaos/ai"] {
        socket
            .send(Message::Binary(rpc_call(id, "readdir", directory).into()))
            .await
            .unwrap();
        let wanted = id;
        id += 1;
        loop {
            let Some(Ok(Message::Binary(bytes))) = socket.next().await else {
                panic!("expected RPC response for {directory}");
            };
            if bytes.len() < 13 {
                continue;
            }
            let frame_len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
            let (header, body) = read_message(&bytes[13..13 + frame_len]).unwrap();
            let RpcValue::Json(Value::Array(header)) = header else {
                continue;
            };
            if header.get(1).and_then(Value::as_u64) != Some(u64::from(wanted)) {
                continue;
            }
            let result_type = header.first().and_then(Value::as_u64).unwrap_or_default();
            assert_eq!(
                result_type,
                rpc_type::PROMISE_SUCCESS as u64,
                "readdir {directory}: {body:?}"
            );
            let RpcValue::Json(entries) = body else {
                panic!("expected json listing")
            };
            assert!(
                entries.as_array().is_some_and(|v| !v.is_empty()),
                "empty listing: {directory}"
            );
            break;
        }
    }
    socket
        .send(Message::Binary(
            rpc_call(
                12,
                "readTextFile",
                "/home/chaos/hunt/dev/projects/chaos-code/Cargo.toml",
            )
            .into(),
        ))
        .await
        .unwrap();
    loop {
        let Some(Ok(Message::Binary(bytes))) = socket.next().await else {
            panic!("expected workspace file read result");
        };
        if bytes.len() < 13 {
            continue;
        }
        let frame_len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
        let (header, body) = read_message(&bytes[13..13 + frame_len]).unwrap();
        let RpcValue::Json(Value::Array(header)) = header else {
            continue;
        };
        if header.get(1).and_then(Value::as_u64) != Some(12) {
            continue;
        }
        let result_type = header.first().and_then(Value::as_u64).unwrap_or_default();
        assert_eq!(
            result_type,
            rpc_type::PROMISE_SUCCESS as u64,
            "workspace read: {body:?}"
        );
        let RpcValue::Json(value) = body else {
            panic!("expected file json")
        };
        assert!(
            value["content"]
                .as_str()
                .unwrap_or_default()
                .contains("[workspace")
        );
        break;
    }
    socket.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn authenticated_file_rpc_uses_home_root_for_project_picker() {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

    fn call(id: u32, method: &str, path: &str) -> Vec<u8> {
        write_frame(&write_message(
            &RpcValue::Json(json!([rpc_type::PROMISE, id, "file", method])),
            &RpcValue::Json(json!({ "path": path })),
        ))
    }

    let root = std::path::PathBuf::from("/home/chaos");
    let target = root.join("hunt/dev/projects/chaos-code");
    assert!(target.is_dir());
    let temp = tempdir().unwrap();
    let auth = WebAuth::disabled();
    let app = router_with_auth_and_previews_and_file_root(
        Engine::new(),
        "",
        false,
        None,
        PreviewConfig::disabled(),
        auth,
        Some(root.clone()),
    );
    let (origin, server) = start(app).await;
    let (mut socket, _) = connect_async(format!(
        "ws://{}/ws/zcode",
        origin.trim_start_matches("http://")
    ))
    .await
    .unwrap();
    for (id, method, path) in [
        (11, "readdir", root.to_string_lossy().to_string()),
        (12, "readdir", target.to_string_lossy().to_string()),
        (
            13,
            "readTextFile",
            target.join("Cargo.toml").to_string_lossy().to_string(),
        ),
    ] {
        socket
            .send(Message::Binary(call(id, method, &path).into()))
            .await
            .unwrap();
        loop {
            let Some(Ok(Message::Binary(bytes))) = socket.next().await else {
                panic!("file RPC closed")
            };
            if bytes.len() < 13 {
                continue;
            }
            let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
            let (header, body) = read_message(&bytes[13..13 + len]).unwrap();
            let RpcValue::Json(Value::Array(header)) = header else {
                continue;
            };
            if header.get(1).and_then(Value::as_u64) != Some(u64::from(id)) {
                continue;
            }
            assert_eq!(
                header.first().and_then(Value::as_u64),
                Some(u64::from(rpc_type::PROMISE_SUCCESS)),
                "{method} {path}: {body:?}"
            );
            let RpcValue::Json(value) = body else {
                panic!("expected json response")
            };
            if method == "readTextFile" {
                assert!(value["content"].as_str().unwrap().contains("[workspace]"));
            } else {
                assert!(!value.as_array().unwrap().is_empty());
            }
            break;
        }
    }
    socket.close(None).await.unwrap();
    server.abort();
    let _ = temp;
}
