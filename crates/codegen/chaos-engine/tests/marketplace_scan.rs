use chaos_engine::{ClientMessage, Engine, ServerMessage};
use std::fs;
use tempfile::tempdir;

#[test]
fn marketplace_scan_is_read_only_and_rejects_escaping_root_as_empty_or_invalid_fixture() {
    let directory = tempdir().unwrap();
    fs::create_dir_all(directory.path().join("plugins/demo/skills/one")).unwrap();
    fs::write(
        directory.path().join("plugins/demo/plugin.json"),
        r#"{"name":"demo","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("plugins/demo/skills/one/SKILL.md"),
        "# Demo",
    )
    .unwrap();
    let engine = Engine::new()
        .with_marketplace_root(directory.path())
        .unwrap();
    let result = engine.handle(ClientMessage::ScanMarketplace {
        client_msg_id: "scan".into(),
        root: directory.path().display().to_string(),
    });
    match &result[0] {
        ServerMessage::MarketplaceScan { entries, .. } => {
            assert!(entries.iter().any(|entry| entry.name == "demo"))
        }
        other => panic!("unexpected {other:?}"),
    }
    let rejected = engine.handle(ClientMessage::ScanMarketplace {
        client_msg_id: "rejected".into(),
        root: directory.path().join("../outside").display().to_string(),
    });
    assert!(matches!(
        rejected.as_slice(),
        [ServerMessage::Error { code, .. }] if code == "marketplace_root_not_allowed"
    ));
}
