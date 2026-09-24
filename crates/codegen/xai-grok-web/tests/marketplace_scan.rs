use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::{fs, time::Duration};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn websocket_scans_only_configured_marketplace_roots() {
    let root = tempdir().unwrap();
    let plugin_dir = root.path().join("plugins/demo");
    fs::create_dir_all(plugin_dir.join(".claude-plugin")).unwrap();
    fs::create_dir_all(plugin_dir.join("skills/one")).unwrap();
    fs::write(
        plugin_dir.join(".claude-plugin/plugin.json"),
        r#"{"name":"demo","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(plugin_dir.join("skills/one/SKILL.md"), "# Demo").unwrap();
    let allowed_root = root.path().to_path_buf();
    let outside_root = tempdir().unwrap();
    let engine = Engine::new().with_marketplace_root(&allowed_root).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(500)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _handshake = socket.next().await.unwrap().unwrap();

    for (id, path) in [
        ("allowed", allowed_root),
        ("denied", outside_root.path().to_path_buf()),
    ] {
        socket
            .send(Message::Text(
                serde_json::to_string(&ClientMessage::ScanMarketplace {
                    client_msg_id: id.into(),
                    root: path.display().to_string(),
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let response: ServerMessage =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        match (id, response) {
            ("allowed", ServerMessage::MarketplaceScan { entries, .. }) => {
                assert!(entries.iter().any(|entry| {
                    entry.get("name").and_then(serde_json::Value::as_str) == Some("demo")
                }));
            }
            ("denied", ServerMessage::Error { code, .. }) => {
                assert_eq!(code, "marketplace_root_not_allowed");
            }
            (_, other) => panic!("unexpected response {other:?}"),
        }
    }
    server.await.unwrap();
}
