//! Phase 3b 的证据：界面上对镜像会话做的置顶与归档，必须在宿主重启之后仍然
//! 成立。引擎自己的状态文件是这份记忆的家——CLI 存储没有「置顶」概念，把
//! 标志写回 CLI 文件反而是污染别人的真相。
//!
//! 流程：建一个 CLI 库 fixture → 引擎 A（带持久化）导入并置顶 + 归档 → 引擎 B
//! （同一路径重启）再导入 → 两行的标志必须原样。

use chaos_engine::chaos_store::scan_store;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use std::fs;
use std::path::Path;

const ENCODED: &str = "%2Fhome%2Fchaos%2Fai";
const SESSION: &str = "01990000-0000-7000-8000-000000000001";
const CWD: &str = "/home/chaos/ai";

fn write_store(root: &Path) {
    let dir = root.join(ENCODED).join(SESSION);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("summary.json"),
        serde_json::json!({
            "info": {"id": SESSION, "cwd": CWD},
            "session_summary": "终端里的那段对话",
            "updated_at": "2026-10-12T01:00:00Z",
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.join("chat_history.jsonl"),
        "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_query>帮我看看</user_query>\"}]}\n",
    )
    .unwrap();
    fs::write(dir.join("updates.jsonl"), "").unwrap();
}

#[test]
fn pin_and_archive_survive_a_host_restart_for_mirrored_sessions() {
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join("sessions");
    write_store(&store);
    let state_path = home.path().join("web-state.json");
    assert_eq!(scan_store(&store, 50).len(), 1, "fixture 可扫出");

    // 引擎 A：导入、置顶、归档（会话状态落盘）。
    {
        let engine = Engine::with_persistence(&state_path)
            .unwrap()
            .with_chaos_store_root(&store);
        engine.import_chaos_sessions(&store);
        let sessions = list_titles(&engine, "probe-a1");
        assert_eq!(sessions.len(), 1, "镜像出一行");
        let id = sessions[0].0.clone();
        engine.handle(ClientMessage::PinSession {
            client_msg_id: "pin-1".into(),
            session_id: uuid::Uuid::parse_str(&id).unwrap(),
            pinned: true,
        });
        engine.handle(ClientMessage::ArchiveSession {
            client_msg_id: "archive-1".into(),
            session_id: uuid::Uuid::parse_str(&id).unwrap(),
            archived: true,
        });
        // 侧栏徽标：模拟一轮回答的未读（直接构造不行——unread_at 由 deliver
        // 置上；这里经一次真实 demo 轮次后检查，重启后的存活与清除一并覆盖）。
        let engine2 = Engine::with_persistence(&state_path)
            .unwrap()
            .with_chaos_store_root(&store);
        engine2.import_chaos_sessions(&store);
        // 触发一次落盘：任何 handle 都 persist，这里读一次列表确保状态写出。
        let _ = engine.handle(ClientMessage::ListSessions {
            client_msg_id: "list-1".into(),
            workspace_id: None,
        });
        assert!(state_path.exists(), "状态文件必须已写出");
        let persisted = fs::read_to_string(&state_path).unwrap();
        eprintln!(
            "状态文件前 400 字节：{}",
            &persisted[..persisted.len().min(400)]
        );
        let value: serde_json::Value = serde_json::from_str(&persisted).unwrap();
        eprintln!(
            "envelope schema_version: {:?}  sessions 数: {:?}",
            value.get("schema_version"),
            value
                .get("state")
                .and_then(|s| s.get("sessions"))
                .map(|x| x.as_object().map(|o| o.len()))
        );
        eprintln!(
            "首个会话的字段: {:?}",
            value
                .get("state")
                .and_then(|s| s.get("sessions"))
                .and_then(|x| x.as_object())
                .and_then(|o| o.values().next())
                .and_then(|v| v.as_object())
                .map(|o| (
                    o.keys().collect::<Vec<_>>(),
                    o.get("pinned"),
                    o.get("archived"),
                    o.get("imported").map(|i| i.is_null())
                ))
        );
    }

    // 引擎 B：同一路径重启。置顶与归档必须还在——这是「写回」真正需要的
    // 持久性，而不是把标志写进 CLI 的文件。
    {
        let engine = Engine::with_persistence(&state_path)
            .unwrap()
            .with_chaos_store_root(&store);

        engine.import_chaos_sessions(&store);
        let sessions = list_titles(&engine, "probe-b2");
        assert_eq!(sessions.len(), 1, "重启后镜像仍是一行");
        let (_, title, pinned, archived) = &sessions[0];
        assert!(pinned, "置顶必须跨重启存活，实际 title={title}");
        assert!(archived, "归档必须跨重启存活，实际 title={title}");
    }
}

#[test]
fn unread_timestamp_marks_on_output_clears_on_seen_and_survives_restart() {
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join("sessions");
    write_store(&store);
    let state_path = home.path().join("web-state.json");

    let engine = Engine::with_persistence(&state_path)
        .unwrap()
        .with_chaos_store_root(&store);
    engine.import_chaos_sessions(&store);
    let id = list_titles(&engine, "unread-a1")[0].0.clone();
    let session_id = uuid::Uuid::parse_str(&id).unwrap();
    // 一轮 demo 回答：首个文本增量置上未读时间戳。
    engine
        .handle(ClientMessage::Submit {
            client_msg_id: "unread-ask".into(),
            session_id,
            prompt: "打个招呼".into(),
        })
        .iter()
        .for_each(|_| {});
    let mut spin = 0;
    while engine.session_is_running(session_id) && spin < 200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        spin += 1;
    }
    let row = list_titles_full(&engine, "unread-a2").remove(0);
    assert!(row.2.is_some(), "一轮回答之后侧栏必须有未读时间戳");

    // 重启：时间戳还在（与置顶/归档同一条持久化路径）。
    drop(engine);
    let engine_b = Engine::with_persistence(&state_path)
        .unwrap()
        .with_chaos_store_root(&store);
    engine_b.import_chaos_sessions(&store);
    let row = list_titles_full(&engine_b, "unread-b1").remove(0);
    assert!(row.2.is_some(), "未读时间戳必须跨重启存活");

    // 宿主打开会话：MarkSessionSeen 清掉。
    engine_b.handle(ClientMessage::MarkSessionSeen {
        client_msg_id: "unread-seen".into(),
        session_id,
    });
    let row = list_titles_full(&engine_b, "unread-b2").remove(0);
    assert!(row.2.is_none(), "打开会话之后必须没有未读");
}

/// (session_id, title, pinned, archived) 的读法，与桥的列表同一来源。
/// client_msg_id 必须每次唯一：引擎按 id 去重重放，同 id 的第二次查询只回 Ack。
fn list_titles_full(engine: &Engine, probe: &str) -> Vec<(String, String, Option<u64>, bool)> {
    engine
        .handle(ClientMessage::ListSessions {
            client_msg_id: probe.into(),
            workspace_id: None,
        })
        .into_iter()
        .find_map(|event| match event {
            ServerMessage::Sessions { sessions, .. } => Some(
                sessions
                    .iter()
                    .map(|info| {
                        (
                            info.session_id.to_string(),
                            info.title.clone(),
                            info.unread_at,
                            info.archived,
                        )
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[allow(dead_code)]
fn list_titles(engine: &Engine, probe: &str) -> Vec<(String, String, bool, bool)> {
    engine
        .handle(ClientMessage::ListSessions {
            client_msg_id: probe.into(),
            workspace_id: None,
        })
        .into_iter()
        .find_map(|event| match event {
            ServerMessage::Sessions { sessions, .. } => Some(
                sessions
                    .iter()
                    .map(|info| {
                        (
                            info.session_id.to_string(),
                            info.title.clone(),
                            info.pinned,
                            info.archived,
                        )
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}
