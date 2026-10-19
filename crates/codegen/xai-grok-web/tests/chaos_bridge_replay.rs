//! End-to-end bridge replay: the 32 frames a **real** official ZCode frontend
//! sent during its boot session (`handshake-capture.json`) are driven, byte for
//! byte, through chaos-web's `/ws`, and the answers are checked against the
//! boot contract the official server honoured in the same capture.
//!
//! This is the strongest offline proof that the official UI can boot against
//! the Chaos backend: the exact bytes the browser produced produce the exact
//! answer sequence it expects, with no Node.js in the loop.

use std::time::Duration;

use chaos_engine::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type};

/// The capture lives with the protocol crate's golden fixtures on purpose: one
/// recording, two consumers (protocol unit tests, host replay test).
const CAPTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../common/zcode-rpc-protocol/tests/fixtures/handshake-capture.json"
));

fn captured_frames() -> Vec<(bool, Vec<u8>)> {
    let frames: Value = serde_json::from_str(CAPTURE).expect("capture parses");
    frames
        .as_array()
        .expect("array")
        .iter()
        .map(|entry| {
            let hex = entry["hex"].as_str().expect("hex");
            (
                entry["dir"].as_str() == Some("C->S"),
                (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("byte"))
                    .collect::<Vec<u8>>(),
            )
        })
        .collect()
}

fn inner(bytes: &[u8]) -> Vec<u8> {
    let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    bytes[13..13 + len].to_vec()
}

fn header_of(payload: &[u8]) -> Vec<Value> {
    let (header, _) = read_message(payload).unwrap_or_else(|reason| {
        panic!(
            "message parses ({}B, head {}): {reason:?}",
            payload.len(),
            &payload[..payload.len().min(16)]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    });
    match header {
        RpcValue::Json(Value::Array(items)) => items,
        other => panic!("array header expected, got {other:?}"),
    }
}

fn body_of(payload: &[u8]) -> Value {
    let (_, body) = read_message(payload).expect("message parses");
    match body {
        RpcValue::Json(value) => value,
        RpcValue::Undefined => Value::Null,
        RpcValue::Binary(bytes, _) => serde_json::json!({ "binary": bytes }),
    }
}

/// 抓包对拍拿的是官方那台机器上的 `en-US`，而 `setting.get` 读的是
/// `$HOME/.chaos/web/zcode_settings.json`。这台机器上只要真在浏览器里打开过一次页面，
/// 那个文件里就留下 `locale: "zh-CN"`，对拍立刻变成假失败——机器的状态不能决定
/// 抓包对拍的结果，所以先把 HOME 换成一个空的临时目录。
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

async fn serve() -> std::net::SocketAddr {
    isolate_settings_home();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(30)).await })
            .await;
    });
    address
}

#[tokio::test]
async fn official_boot_bytes_get_official_answer_sequence() {
    let address = serve().await;
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();

    // 1. A real browser does not block on the server greeting: the official
    //    client fires its first channel calls the moment the socket opens.
    //    Send the first captured client frame immediately — this is also what
    //    the protocol sniffer needs to pick the ZCode side.
    let frames = captured_frames();
    let client_frames: Vec<Vec<u8>> = frames
        .iter()
        .filter(|(c, _)| *c)
        .map(|(_, bytes)| bytes.clone())
        .collect();
    socket
        .send(Message::Binary(client_frames[0].clone().into()))
        .await
        .unwrap();

    // 2. The server speaks first with the ZCode Initialize — as a BINARY frame,
    //    proving the sniffer picked the ZCode protocol, not the JSON Handshake.
    let greeting = socket.next().await.unwrap().unwrap();
    let Message::Binary(greeting_wire) = &greeting else {
        panic!("expected binary ZCode Initialize, got {greeting:?}");
    };
    let greeting_payload = inner(greeting_wire.as_ref());
    let (header, body) = read_message(&greeting_payload).unwrap();
    assert_eq!(
        header,
        RpcValue::Json(serde_json::json!([rpc_type::INITIALIZE]))
    );
    assert_eq!(body, RpcValue::Undefined);

    // 3. Replay the remaining captured client frames, byte for byte, in order.
    for bytes in client_frames.iter().skip(1) {
        socket
            .send(Message::Binary(bytes.clone().into()))
            .await
            .unwrap();
    }

    // 4. Collect the answers: one PromiseSuccess per one-shot call. Listener
    //    registrations answer nothing.
    let mut payload_sequence: Vec<Vec<u8>> = Vec::new();
    while payload_sequence.len() < 24 {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("answers arrive within 5s")
            .expect("socket alive")
            .expect("frame ok");
        let Message::Binary(wire) = message else {
            panic!("binary answers only, got {message:?}");
        };
        payload_sequence.push(inner(wire.as_ref()));
    }

    // 5. The [type, id] answer SET equals the official capture. The official
    //    server answers in asynchronous completion order, the bridge in frame
    //    order, so only the set is contractual.
    // The Initialize is consumed as the greeting above, so the set compares
    // only the PromiseSuccess answers.
    let captured_server: Vec<Vec<Value>> = frames
        .iter()
        .filter(|(c, _)| !*c)
        .map(|(_, bytes)| header_of(inner(bytes).as_slice()))
        .filter(|header| header[0] != serde_json::json!(rpc_type::INITIALIZE))
        .collect();
    let ours: Vec<Vec<Value>> = payload_sequence.iter().map(|p| header_of(p)).collect();
    let key = |headers: &[Vec<Value>]| -> Vec<String> {
        let mut keys: Vec<String> = headers
            .iter()
            .map(|h| serde_json::to_string(h).expect("json key"))
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(
        key(&ours),
        key(&captured_server),
        "answer type/id set must match the official server"
    );

    // 5. The boot contract's load-bearing values.
    let answers: std::collections::HashMap<u64, Value> = payload_sequence
        .iter()
        .map(|p| {
            let header = header_of(p);
            (header[1].as_u64().expect("id"), body_of(p))
        })
        .collect();
    assert_eq!(
        answers[&20],
        serde_json::json!(false),
        "shouldOnboard: skip the welcome wall"
    );
    for id in [1u64, 10, 16, 26] {
        assert_eq!(
            answers[&id]["locale"],
            serde_json::json!("en-US"),
            "setting.get #{id}"
        );
        assert!(answers[&id].get("recentProjects").is_some());
    }
    assert_eq!(
        answers[&4],
        serde_json::json!({ "mode": "alwaysOn", "enabled": true, "source": "remote" })
    );
    for id in [6u64, 25] {
        let view = &answers[&id];
        assert_eq!(
            view["revision"],
            serde_json::json!(1),
            "model-selection revision"
        );
        let providers = view["providers"].as_array().expect("Chaos model providers");
        assert!(
            !providers.is_empty(),
            "model selection must expose Chaos config models"
        );
        assert_eq!(providers[0]["providerId"], "account:chaos");
        assert!(
            providers[0]["models"]
                .as_array()
                .is_some_and(|models| !models.is_empty())
        );
        assert_eq!(view["selection"]["providerId"], "account:chaos");
    }
    for id in [9u64, 13, 21, 27] {
        assert_eq!(
            answers[&id]["revision"],
            serde_json::json!(1),
            "provider-settings #{id}"
        );
        assert_eq!(
            answers[&id]["providerTemplates"].as_array().map(Vec::len),
            Some(1)
        );
    }
    assert_eq!(
        answers[&14]["status"],
        serde_json::json!("authenticated"),
        "local bridge session"
    );
    assert_eq!(
        answers[&14]["userInfo"]["displayName"],
        serde_json::json!("Chaos")
    );
    for id in [28u64, 29] {
        let providers = answers[&id].as_array().expect("providers array");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["id"], serde_json::json!("chaos"));
    }
    for id in [18u64, 19, 23, 24] {
        assert_eq!(
            answers[&id],
            Value::Null,
            "credential/oauth/agent answer null for #{id}"
        );
    }
    for id in [22u64, 30, 31] {
        assert_eq!(
            answers[&id],
            Value::String("chaos".into()),
            "active local provider #{id}"
        );
    }
    for id in [2u64, 3] {
        assert_eq!(answers[&id], Value::Null, "onboarding-record #{id}");
    }
}

#[tokio::test]
async fn json_protocol_still_gets_its_handshake_after_the_sniffer() {
    // A client that speaks text is the Chaos JSON protocol: it must still see
    // the text Handshake as the very first frame, before anything else.
    let address = serve().await;
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let first = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = first else {
        panic!("JSON clients must get the text Handshake first, got {first:?}");
    };
    assert!(
        text.contains("protocol_version"),
        "handshake payload: {text}"
    );
}

#[tokio::test]
async fn unknown_channel_method_is_an_honest_promise_error() {
    let address = serve().await;
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();

    // Like a real ZCode client: fire the call without waiting for the greeting
    // (inside the sniffer's grace window), then read the server's Initialize.
    let call_header = serde_json::json!([rpc_type::PROMISE, 77, "nonexistent-channel", "ping"]);
    let mut call_frame = zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(call_header));
    call_frame.extend_from_slice(&zcode_rpc_protocol::serialize_chunk(&RpcValue::Undefined));
    socket
        .send(Message::Binary(
            zcode_rpc_protocol::write_frame(&call_frame).into(),
        ))
        .await
        .unwrap();

    let greeting = socket.next().await.unwrap().unwrap();
    let Message::Binary(greeting_wire) = &greeting else {
        panic!("expected binary ZCode Initialize, got {greeting:?}");
    };
    let (greeting_header, _) = read_message(inner(greeting_wire.as_ref()).as_slice()).unwrap();
    assert_eq!(
        greeting_header,
        RpcValue::Json(serde_json::json!([rpc_type::INITIALIZE]))
    );

    // The unknown-channel call was buffered; it expires into an honest error.
    let answer = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("answer within 5s")
        .unwrap()
        .unwrap();
    let Message::Binary(wire) = answer else {
        panic!("binary answer")
    };
    let (header, body) = read_message(inner(wire.as_ref()).as_slice()).unwrap();
    assert_eq!(
        header,
        RpcValue::Json(serde_json::json!([rpc_type::PROMISE_ERROR, 77]))
    );
    match body {
        RpcValue::Json(value) => {
            // The official server's answer for a channel that never registers:
            // buffered for the timeout, then this exact error shape.
            assert_eq!(value["name"], serde_json::json!("Unknown channel"));
            let message = value["message"].as_str().expect("message");
            assert!(
                message.contains("nonexistent-channel"),
                "message: {message}"
            );
            assert!(
                message.contains("timed out after 1000ms"),
                "message: {message}"
            );
        }
        other => panic!("error object expected, got {other:?}"),
    }
}

/// Drive one Promise over `/ws/zcode` and return `(response type, body)` —
/// mirroring how the official web shell's `@zcode/rpc` client calls a channel.
async fn call_over_dedicated_route(
    address: std::net::SocketAddr,
    id: u32,
    channel: &str,
    method: &str,
    arg: Value,
) -> (u64, Value) {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .unwrap();
    let mut frame = zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(serde_json::json!([
        rpc_type::PROMISE,
        id,
        channel,
        method
    ])));
    frame.extend_from_slice(&zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(arg)));
    socket
        .send(Message::Binary(
            zcode_rpc_protocol::write_frame(&frame).into(),
        ))
        .await
        .unwrap();

    // The greeting is the Initialize; the answer for this id follows. Any other
    // frame (notably an `Unknown channel` PromiseError for this id) is surfaced
    // verbatim by the assertions in the caller.
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("answer within 5s")
            .expect("socket alive")
            .expect("frame ok");
        let Message::Binary(wire) = message else {
            panic!("binary answer expected")
        };
        let (header, body) = read_message(inner(wire.as_ref()).as_slice()).unwrap();
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.len() < 2 || items[1].as_u64() != Some(u64::from(id)) {
            continue; // the Initialize carries no id
        }
        let kind = items[0].as_u64().expect("response type");
        let body = match body {
            RpcValue::Json(value) => value,
            RpcValue::Undefined => Value::Null,
            RpcValue::Binary(bytes, _) => serde_json::json!({ "binary": bytes }),
        };
        return (kind, body);
    }
}

/// The settings page's 浏览器控制 switch and the 插件 section share one store that
/// calls `plugin-management.listPlugins` + `.getPluginsOverview` the moment a
/// workspace opens. While that channel was absent from `known_channels()` both
/// calls expired into the raw protocol error
/// `Channel name 'plugin-management' timed out after 1000ms`, which the store
/// rendered under the switch. The channel now answers the real contract.
#[tokio::test]
async fn plugin_management_channel_answers_the_settings_page_contract() {
    let address = serve().await;

    let (kind, list) = call_over_dedicated_route(
        address,
        41,
        "plugin-management",
        "listPlugins",
        serde_json::json!({ "workspacePath": "/" }),
    )
    .await;
    assert_eq!(
        kind,
        rpc_type::PROMISE_SUCCESS as u64,
        "listPlugins must not time out: {list}"
    );
    assert_eq!(
        list["plugins"],
        serde_json::json!([]),
        "Chaos installs no ZCode plugins"
    );
    assert_eq!(list["diagnostics"], serde_json::json!([]));

    let (kind, overview) = call_over_dedicated_route(
        address,
        42,
        "plugin-management",
        "getPluginsOverview",
        serde_json::json!({ "workspacePath": "/" }),
    )
    .await;
    assert_eq!(
        kind,
        rpc_type::PROMISE_SUCCESS as u64,
        "getPluginsOverview must not time out"
    );
    // The upstream result schema requires every collection plus `capability`;
    // a missing key would be rejected client-side and look like a broken server.
    for key in [
        "marketplaces",
        "availablePlugins",
        "installedPlugins",
        "restorableBuiltins",
        "diagnostics",
    ] {
        assert_eq!(overview[key], serde_json::json!([]), "overview.{key}");
    }
    assert_eq!(
        overview["capability"]["supported"],
        serde_json::json!(false)
    );

    // A method the Chaos engine genuinely cannot serve stays an honest error —
    // fast, and named — instead of the 1000 ms unknown-channel timeout.
    let (kind, error) = call_over_dedicated_route(
        address,
        43,
        "plugin-management",
        "setPluginEnabled",
        serde_json::json!({ "workspacePath": "/", "pluginId": "browser-use@zcode-plugins-official", "enabled": true }),
    )
    .await;
    assert_eq!(
        kind,
        rpc_type::PROMISE_ERROR as u64,
        "unsupported mutation stays an error"
    );
    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("plugin-management.setPluginEnabled"),
        "message: {message}"
    );
    assert!(
        !message.contains("timed out"),
        "must not be the unknown-channel timeout: {message}"
    );
}

#[tokio::test]
async fn git_refresh_returns_an_object_over_zcode_rpc() {
    let workspace = tempfile::tempdir().expect("temporary repository");
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace.path())
        .args(["init", "-q"])
        .status()
        .expect("git init starts");
    assert!(init.success(), "git init failed");

    let address = serve().await;
    let (kind, result) = call_over_dedicated_route(
        address,
        44,
        "git",
        "refresh",
        serde_json::json!({ "workspacePath": workspace.path().to_string_lossy() }),
    )
    .await;
    assert_eq!(kind, rpc_type::PROMISE_SUCCESS as u64);
    assert!(result.is_object(), "Git.refresh fulfilled with {result}");
    assert!(result["summary"].is_object());
    assert!(result["unstagedChanges"].is_array());
    assert!(result["stagedChanges"].is_array());
}

fn is_listener(channel: &str, name: &str) -> bool {
    matches!(
        (channel, name),
        ("broadcast", "onMessage")
            | ("model-selection", "onDidChange")
            | ("provider-settings", "onDidChange")
    )
}

#[tokio::test]
async fn every_bridge_answer_encodes_as_one_complete_rpc_message() {
    // Every (channel, method) the real boot session exercised must encode as
    // exactly one header chunk + one body chunk with nothing trailing.
    use zcode_rpc_protocol::RpcValue as V;
    let calls: Vec<(&str, &str, V)> = vec![
        ("broadcast", "onMessage", V::Undefined),
        ("setting", "get", V::Undefined),
        ("onboarding-record", "claimAnonymousRecord", V::Undefined),
        ("onboarding-record", "getLatestEntry", V::Undefined),
        ("onboarding-record", "shouldOnboard", V::Undefined),
        (
            "coding-plan-subscription",
            "getDynamicWorkflowClientConfig",
            V::Undefined,
        ),
        ("model-selection", "onDidChange", V::Undefined),
        ("model-selection", "getView", V::Undefined),
        ("provider-settings", "onDidChange", V::Undefined),
        ("provider-settings", "getView", V::Undefined),
        ("provider-settings", "refresh", V::Undefined),
        ("oauth", "restoreCachedSessionState", V::Undefined),
        ("oauth", "getActiveProvider", V::Undefined),
        ("oauth", "getProviders", V::Undefined),
        ("credential", "load", V::Undefined),
        ("zcode-agent", "syncAppRuntimePreferences", V::Undefined),
        ("bots", "syncAppRuntimePreferences", V::Undefined),
        ("plugin-management", "listPlugins", V::Undefined),
        ("plugin-management", "getPluginsOverview", V::Undefined),
        (
            "plugin-management",
            "getPluginReferenceCatalog",
            V::Undefined,
        ),
    ];
    for (index, (channel, name, arg)) in calls.iter().enumerate() {
        let id = index as u32 + 1;
        let value =
            if zcode_rpc_protocol::rpc_type::EVENT_LISTEN == 102 && is_listener(channel, name) {
                // Listen channels never travel through handle_call; skip them.
                continue;
            } else {
                xai_grok_web::chaos_bridge::handle_call(channel, name, &arg)
                    .unwrap_or_else(|reason| panic!("{channel}.{name}: {reason}"))
            };
        let frame = zcode_rpc_protocol::ChannelServer::promise_success_frame(id, &value);
        match read_message(&frame) {
            Ok((header, body)) => {
                assert_eq!(
                    header,
                    RpcValue::Json(serde_json::json!([rpc_type::PROMISE_SUCCESS, id]))
                );
                assert_ne!(body, RpcValue::Undefined, "{channel}.{name} has a body");
            }
            Err(reason) => panic!("{channel}.{name} id={id}: {reason:?}"),
        }
    }
}

/// The settings 技能 panel and the sidebar badge call `skills.list` the moment
/// a workspace opens, and the composer `$` panel routes through
/// `zcode-agent.getSkillReferenceCatalog`. While the channel was absent from
/// `known_channels()` both expired into the raw protocol error
/// `Channel name 'skills' timed out after 1000ms`, which the panels rendered
/// verbatim. The channel now answers the real contract, listing what the
/// agent's own discovery finds in the workspace.
#[tokio::test]
async fn skills_channel_answers_the_settings_contract() {
    let address = serve().await;
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let skill_dir = workspace
        .path()
        .join(".grok")
        .join("skills")
        .join("chaos-demo");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: chaos-demo\ndescription: 用于回放的技能\n---\n\n正文\n",
    )
    .expect("SKILL.md");

    let (kind, list) = call_over_dedicated_route(
        address,
        45,
        "skills",
        "list",
        serde_json::json!({
            "workspacePath": workspace.path().to_string_lossy(),
            "provider": "zcode-agent",
        }),
    )
    .await;
    assert_eq!(
        kind,
        rpc_type::PROMISE_SUCCESS as u64,
        "skills.list must not time out: {list}"
    );
    let demo = list["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .find(|skill| skill["id"] == "chaos-demo")
        .expect("discovered chaos-demo skill");
    assert_eq!(demo["name"], serde_json::json!("chaos-demo"));
    assert_eq!(demo["description"], serde_json::json!("用于回放的技能"));
    assert_eq!(demo["scope"], serde_json::json!("workspace"));
    assert_eq!(demo["enabled"], serde_json::json!(true));
    assert_eq!(list["diagnostics"], serde_json::json!([]));
    // 合同要求 capability 存在；Web 桥不落盘写技能目录，导入入口按桌面端专用处理。
    assert_eq!(list["capability"]["supported"], serde_json::json!(true));
    assert_eq!(
        list["capability"]["userScopeAvailable"],
        serde_json::json!(false)
    );

    // The composer `$` panel's routing word answers from the same discovery,
    // filtered to enabled skills with the strict referenceCatalog shape.
    let (kind, catalog) = call_over_dedicated_route(
        address,
        46,
        "zcode-agent",
        "getSkillReferenceCatalog",
        serde_json::json!({ "workspacePath": workspace.path().to_string_lossy() }),
    )
    .await;
    assert_eq!(kind, rpc_type::PROMISE_SUCCESS as u64, "catalog: {catalog}");
    assert_eq!(catalog["authority"], serde_json::json!("workspace"));
    let demo = catalog["skills"]
        .as_array()
        .expect("catalog skills")
        .iter()
        .find(|skill| skill["id"] == "chaos-demo")
        .expect("catalog chaos-demo skill");
    assert_eq!(demo["enabled"], serde_json::json!(true));

    // 已知之外的 skills 方法快、有名地拒绝，而不是 unknown-channel 超时。
    let (kind, error) = call_over_dedicated_route(
        address,
        47,
        "skills",
        "deleteSkill",
        serde_json::json!({
            "workspacePath": workspace.path().to_string_lossy(),
            "skillId": "chaos-demo",
        }),
    )
    .await;
    assert_eq!(kind, rpc_type::PROMISE_ERROR as u64, "unsupported mutation");
    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("skills.deleteSkill") && !message.contains("timed out"),
        "message: {message}"
    );
}
