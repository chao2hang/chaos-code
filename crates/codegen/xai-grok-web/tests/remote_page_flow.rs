//! 手机远控页（`/remote`）的宿主合同。
//!
//! 页面本体是自包含 HTML，直接在 `/ws` 上说 Chaos JSON 协议。这里钉住它依赖
//! 的三件事：网关对 HTML 导航的接住与放行、页面自身可服务、以及它启动时的
//! 消息序列（list_workspaces/list_sessions/create_session/submit/resume）在
//! 引擎上得到的回答形状与渲染所需字段。

use std::{net::SocketAddr, time::Duration};

use axum::Router;
use chaos_engine::{Engine, PromptAdapter, RunOptions, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::{
    preview::PreviewConfig, router, router_with_auth_and_previews_and_file_root,
    web_auth::WebAuth,
};

struct TestAgent;

impl PromptAdapter for TestAgent {
    fn run_prompt(&self, prompt: &str, _options: &RunOptions) -> Result<Vec<String>, String> {
        Ok(vec![format!("agent:{prompt}")])
    }
}

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

fn authed_app(temp: &std::path::Path) -> Router {
    let assets = temp.join("assets");
    std::fs::create_dir_all(assets.join("assets")).unwrap();
    std::fs::write(assets.join("index.html"), "official zcode shell").unwrap();
    let auth = WebAuth::open(temp.join("auth/web_auth.json")).unwrap();
    router_with_auth_and_previews_and_file_root(
        Engine::new(),
        "",
        false,
        Some(assets),
        PreviewConfig::disabled(),
        auth,
        Some(std::path::PathBuf::from("/tmp")),
    )
}

#[tokio::test]
async fn remote_page_is_password_gated_then_served() {
    let temp = tempdir().unwrap();
    let app = authed_app(temp.path());
    let (origin, server) = start(app).await;
    let client = reqwest::Client::new();

    // 未登录的 HTML 导航被网关以内联登录页接住——手机浏览器首次打开 /remote
    // 时看到的是它，而不是 401。
    let gated = client
        .get(format!("{origin}/remote"))
        .header("accept", "text/html")
        .send()
        .await
        .unwrap();
    assert_eq!(gated.status(), reqwest::StatusCode::OK);
    assert!(gated.text().await.unwrap().contains("登录"));

    // 首次设置 + 登录，拿到 Cookie。
    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({"password": "remote-test-passphrase", "confirmation": "remote-test-passphrase"}))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);
    let cookie = setup
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    // 登录后 /remote 服务手机页本体：自包含、带 CSP、含协议标记。
    let page = client
        .get(format!("{origin}/remote"))
        .header("accept", "text/html")
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), reqwest::StatusCode::OK);
    assert_eq!(
        page.headers()
            .get(reqwest::header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .contains("connect-src 'self'"),
        true,
        "远控页必须带 CSP"
    );
    let body = page.text().await.unwrap();
    for marker in [
        "Chaos 远控",
        "viewport-fit=cover",
        "id=\"composer\"",
        "list_sessions",
        "tool_approval_requested",
        "respond_question",
    ] {
        assert!(body.contains(marker), "手机页缺少标记 {marker}");
    }
    assert!(!body.contains("min-w-[320px]"), "手机页不得复刻桌面宽度陷阱");
    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}

#[tokio::test]
async fn mobile_boot_sequence_is_answered_by_the_engine() {
    // 页面启动时依次发：list_workspaces、list_sessions；打开会话时发
    // resume。这里按同一顺序走一遍，钉住回答的形状。
    let (origin, server) = start(router(Engine::with_adapter(TestAgent), "")).await;
    let ws_origin = origin.replacen("http://", "ws://", 1);
    let (mut socket, _) = connect_async(format!("{ws_origin}/ws")).await.unwrap();

    let handshake = socket.next().await.unwrap().unwrap();
    let handshake: ServerMessage =
        serde_json::from_str(&handshake.into_text().unwrap()).unwrap();
    assert!(matches!(handshake, ServerMessage::Handshake { .. }));

    async fn send(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        value: Value,
    ) {
        socket
            .send(Message::Text(serde_json::to_string(&value).unwrap().into()))
            .await
            .unwrap();
    }

    // list_workspaces → workspaces（工作区分组标题的数据源）。
    send(&mut socket, json!({"type":"list_workspaces","client_msg_id":"lw"})).await;
    let mut saw_workspaces = false;
    // list_sessions → sessions（任务首页行的数据源）。
    send(&mut socket, json!({"type":"list_sessions","client_msg_id":"ls"})).await;
    let mut session_list: Option<Value> = None;
    // create_session → session_created（新建任务按钮的入口）。
    send(&mut socket, json!({"type":"create_session","client_msg_id":"cs"})).await;
    let mut session_id = None;
    for _ in 0..12 {
        let frame = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let event: Value = serde_json::from_str(&frame).unwrap();
        match event["type"].as_str().unwrap_or_default() {
            "workspaces" => saw_workspaces = true,
            "sessions" => {
                assert!(event["sessions"].is_array(), "sessions 必须带会话数组");
                session_list = Some(event.clone());
            }
            "session_created" => session_id = Some(event["session_id"].as_str().unwrap().to_owned()),
            _ => {}
        }
        if saw_workspaces && session_list.is_some() && session_id.is_some() {
            break;
        }
    }
    assert!(saw_workspaces, "必须回答 workspaces");
    let list = session_list.expect("必须回答 sessions");
    // 任务首页行读取的字段：标题、消息数、未读、归档位。
    if let Some(first) = list["sessions"].as_array().and_then(|rows| rows.first()) {
        for field in ["session_id", "title", "message_count", "archived"] {
            assert!(
                first.get(field).is_some(),
                "SessionInfo 缺少手机页读取的字段 {field}"
            );
        }
    }
    let session_id = session_id.expect("必须回答 session_created");

    // submit → turn_started / text_delta / completed（会话页的实时流）。
    send(
        &mut socket,
        json!({"type":"submit","client_msg_id":"s1","session_id":session_id,"prompt":"你好"}),
    )
    .await;
    let mut saw_turn = false;
    let mut saw_delta = false;
    let mut saw_completed = false;
    for _ in 0..16 {
        let frame = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let event: Value = serde_json::from_str(&frame).unwrap();
        match event["type"].as_str().unwrap_or_default() {
            "turn_started" => {
                assert_eq!(event["prompt"], "你好", "TurnStarted 必须带本轮用户输入");
                saw_turn = true;
            }
            "text_delta" if event["text"] == "agent:你好" => saw_delta = true,
            "completed" => saw_completed = true,
            _ => {}
        }
        if saw_turn && saw_delta && saw_completed {
            break;
        }
    }
    assert!(saw_turn && saw_delta && saw_completed);

    // resume → session_snapshot（会话页打开/刷新时整卷时间线）。
    send(
        &mut socket,
        json!({"type":"resume","client_msg_id":"r1","session_id":session_id,"workspace_id":null}),
    )
    .await;
    let mut snapshot = None;
    for _ in 0..8 {
        let frame = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let event: Value = serde_json::from_str(&frame).unwrap();
        if event["type"] == "session_snapshot" {
            snapshot = Some(event);
            break;
        }
    }
    let snapshot = snapshot.expect("resume 必须回答 session_snapshot");
    let messages = snapshot["messages"].as_array().expect("快照带消息数组");
    assert!(
        messages.iter().any(|m| m["role"] == "user" && m["text"] == "你好"),
        "快照必须含用户行"
    );
    assert!(
        messages.iter().any(|m| m["role"] == "assistant" && m["text"] == "agent:你好"),
        "快照必须含助手行"
    );
    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}

#[tokio::test]
async fn login_reload_keeps_the_mobile_page_on_remote() {
    // 登录成功页曾经硬跳 `/`，会把手机用户从 /remote 甩进桌面工作区。
    // 页面现在 reload 当前地址——钉住这个行为。
    let temp = tempdir().unwrap();
    let app = authed_app(temp.path());
    let (origin, server) = start(app).await;
    let client = reqwest::Client::new();

    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({"password": "remote-test-passphrase", "confirmation": "remote-test-passphrase"}))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);
    let login_page = client
        .get(format!("{origin}/remote"))
        .header("accept", "text/html")
        .send()
        .await
        .unwrap();
    let _ = login_page.text().await.unwrap();
    // 登录页脚本里的跳转语义。
    let auth_page = client
        .get(format!("{origin}/"))
        .header("accept", "text/html")
        .send()
        .await
        .unwrap();
    let script = auth_page.text().await.unwrap();
    assert!(
        script.contains("location.reload()"),
        "登录成功必须 reload 当前页"
    );
    assert!(
        !script.contains("location.replace('/')"),
        "登录成功不得再硬跳桌面页"
    );
    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}

#[tokio::test]
async fn automatic_device_switching_between_mobile_and_pc() {
    let temp = tempdir().unwrap();
    let app = authed_app(temp.path());
    let (origin, server) = start(app).await;
    let client = reqwest::Client::new();

    // 1. 设置密码并拿到认证 Cookie
    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({"password": "remote-test-passphrase", "confirmation": "remote-test-passphrase"}))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);
    let auth_cookie = setup
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let desktop_ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
    let mobile_ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

    // 2. PC 桌面 UA 访问根路径 /：统一提供响应式页面（index.html），无路由重定向
    let desktop_resp = client
        .get(format!("{origin}/"))
        .header("accept", "text/html")
        .header("cookie", &auth_cookie)
        .header("user-agent", desktop_ua)
        .send()
        .await
        .unwrap();
    assert_eq!(desktop_resp.status(), reqwest::StatusCode::OK);
    let desktop_text = desktop_resp.text().await.unwrap();
    assert!(desktop_text.contains("official zcode shell"), "PC 端访问 / 提供统一工作区页面");

    // 3. 手机端 UA 访问同一个根路径 /：同样直接返回 200 OK 统一页面，保持单 URL 无重定向
    let mobile_resp = client
        .get(format!("{origin}/"))
        .header("accept", "text/html")
        .header("cookie", &auth_cookie)
        .header("user-agent", mobile_ua)
        .send()
        .await
        .unwrap();
    assert_eq!(mobile_resp.status(), reqwest::StatusCode::OK);
    let mobile_text = mobile_resp.text().await.unwrap();
    assert!(mobile_text.contains("official zcode shell"), "手机端访问 / 同样服务统一响应式工作区页面，不发生路由跳转");

    // 4. 显式指定 legacy-remote 模式时提供独立轻量页面
    let legacy_resp = client
        .get(format!("{origin}/?ui=legacy-remote"))
        .header("accept", "text/html")
        .header("cookie", &auth_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(legacy_resp.status(), reqwest::StatusCode::OK);
    assert!(legacy_resp.text().await.unwrap().contains("id=\"composer\""));

    // 5. 访问 /remote 路由也服务独立轻量页面并带协议标记
    let remote_direct = client
        .get(format!("{origin}/remote"))
        .header("accept", "text/html")
        .header("cookie", &auth_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(remote_direct.status(), reqwest::StatusCode::OK);
    assert!(remote_direct.text().await.unwrap().contains("id=\"composer\""));

    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}

#[tokio::test]
async fn official_mobile_client_direct_routing_and_pairing_flow() {
    let temp = tempdir().unwrap();
    let assets = temp.path().join("assets");
    std::fs::create_dir_all(assets.join("assets")).unwrap();
    std::fs::write(assets.join("index.html"), "official desktop shell").unwrap();

    // 安装官方手机端镜像 remote-spa/latest/index.html
    let remote_spa_dir = assets.join("remote-spa/latest");
    std::fs::create_dir_all(&remote_spa_dir).unwrap();
    std::fs::write(
        remote_spa_dir.join("index.html"),
        "<html><head><title>ZCode Mobile Official</title></head><body>official mobile shell</body></html>",
    )
    .unwrap();

    let auth = WebAuth::open(temp.path().join("auth/web_auth.json")).unwrap();
    let app = router_with_auth_and_previews_and_file_root(
        Engine::new(),
        "",
        false,
        Some(assets),
        PreviewConfig::disabled(),
        auth,
        Some(std::path::PathBuf::from("/tmp")),
    );
    let (origin, server) = start(app).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none()) // 不自动重定向，检测重定向与 URL 参数
        .build()
        .unwrap();

    // 1. 设置密码并拿到认证 Cookie
    let setup = client
        .post(format!("{origin}/api/auth/setup"))
        .json(&json!({"password": "remote-test-passphrase", "confirmation": "remote-test-passphrase"}))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), reqwest::StatusCode::OK);
    let auth_cookie = setup
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let mobile_ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

    // 2. 手机端与桌面端访问根路径 /：均直接 200 OK，统一服务同一套响应式工作区，绝不发生 307 重定向
    let mobile_resp = client
        .get(format!("{origin}/"))
        .header("accept", "text/html")
        .header("cookie", &auth_cookie)
        .header("user-agent", mobile_ua)
        .send()
        .await
        .unwrap();
    assert_eq!(mobile_resp.status(), reqwest::StatusCode::OK);
    let body = mobile_resp.text().await.unwrap();
    assert!(body.contains("official desktop shell"), "统一响应式工作区页面在单 URL 交付");

    // 3. 访问 /remote/v4 依然提供官方手机客户端镜像
    let spa_resp = client
        .get(format!("{origin}/remote/v4"))
        .header("accept", "text/html")
        .header("cookie", &auth_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(spa_resp.status(), reqwest::StatusCode::OK);
    let spa_body = spa_resp.text().await.unwrap();
    assert!(spa_body.contains("official mobile shell"));

    server.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
}
