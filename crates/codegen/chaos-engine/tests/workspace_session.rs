use chaos_engine::{ClientMessage, Engine, ServerMessage};
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn resume_rejects_a_session_from_another_workspace() {
    let engine = Engine::new();
    let first = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-first".into(),
        workspace_id: None,
    });
    let (session_id, workspace_id) = match first.as_slice() {
        [
            ServerMessage::SessionCreated {
                session_id,
                workspace_id,
            },
        ] => (*session_id, *workspace_id),
        other => panic!("unexpected {other:?}"),
    };
    let wrong_workspace = Uuid::new_v4();
    let result = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume-wrong".into(),
        session_id,
        workspace_id: Some(wrong_workspace),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::Error { code, .. }] if code == "workspace_session_mismatch"
    ));
    let result = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume-right".into(),
        session_id,
        workspace_id: Some(workspace_id),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::SessionSnapshot { .. }]
    ));
}

#[test]
fn persisted_session_keeps_workspace_binding_after_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("state.json");
    let first = Engine::with_persistence(&path).unwrap();
    let created = first.handle(ClientMessage::CreateSession {
        client_msg_id: "create".into(),
        workspace_id: None,
    });
    let (session_id, workspace_id) = match created.as_slice() {
        [
            ServerMessage::SessionCreated {
                session_id,
                workspace_id,
            },
        ] => (*session_id, *workspace_id),
        other => panic!("unexpected {other:?}"),
    };
    drop(first);
    let second = Engine::with_persistence(&path).unwrap();
    let result = second.handle(ClientMessage::Snapshot {
        client_msg_id: "snapshot".into(),
        session_id,
        workspace_id: Some(workspace_id),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::SessionSnapshot { .. }]
    ));
}

/// `Workspaces` reports `active_workspace_id` as the nil UUID when the host has no
/// active workspace. A client that echoes that placeholder back means "none
/// selected", which is what a reconnecting page sends after a host restart.
#[test]
fn placeholder_active_workspace_id_round_trips_as_no_workspace_selected() {
    let engine = Engine::new();
    let listed = engine.handle(ClientMessage::ListWorkspaces {
        client_msg_id: "list".into(),
    });
    let placeholder = match listed.as_slice() {
        [
            ServerMessage::Workspaces {
                active_workspace_id,
                ..
            },
        ] => *active_workspace_id,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(placeholder, Uuid::nil());

    let created = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-via-placeholder".into(),
        workspace_id: Some(placeholder),
    });
    let session_id = match created.as_slice() {
        [ServerMessage::SessionCreated { session_id, .. }] => *session_id,
        other => panic!("unexpected {other:?}"),
    };
    let resumed = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume-via-placeholder".into(),
        session_id,
        workspace_id: Some(placeholder),
    });
    assert!(
        matches!(resumed.as_slice(), [ServerMessage::SessionSnapshot { .. }]),
        "{resumed:?}"
    );
}

/// Only the placeholder is read as "none"; an unknown id stays a refusal.
#[test]
fn unknown_workspace_id_is_still_refused() {
    let engine = Engine::new();
    let result = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-unknown".into(),
        workspace_id: Some(Uuid::new_v4()),
    });
    assert!(
        matches!(
            result.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "workspace_unavailable"
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn switch_workspace_broadcasts_workspaces_and_advances_sequence() {
    let engine = Engine::new();
    let stream_a = engine.subscribe();
    let mut stream_b = engine.subscribe();

    // 创建第一个会话与工作区
    let created = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-1".into(),
        workspace_id: None,
    });
    let (first_session, first_workspace) = match created.as_slice() {
        [
            ServerMessage::SessionCreated {
                session_id,
                workspace_id,
            },
        ] => (*session_id, *workspace_id),
        other => panic!("unexpected {other:?}"),
    };

    // 创建第二个工作区
    let ws2_events = engine.handle(ClientMessage::CreateWorkspace {
        client_msg_id: "create-ws2".into(),
        name: "工作区 2".into(),
    });
    let second_workspace = ws2_events
        .iter()
        .find_map(|msg| {
            if let ServerMessage::Workspaces { workspaces, .. } = msg {
                workspaces
                    .iter()
                    .find(|w| w.name == "工作区 2")
                    .map(|w| w.id)
            } else {
                None
            }
        })
        .expect("should find 工作区 2");

    // 订阅者 A 发起切换工作区到 second_workspace
    let sink_a = stream_a.sink();
    engine.dispatch_to(
        ClientMessage::SwitchWorkspace {
            client_msg_id: "switch-to-2".into(),
            workspace_id: second_workspace,
        },
        Some(&sink_a),
        &mut |_| {},
    );

    // 验证订阅者 B（另一标签页）也收到了广播的 Workspaces，且活动工作区为 second_workspace
    let mut found_b_broadcast = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(msg)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), stream_b.recv()).await
        {
            if let ServerMessage::Workspaces {
                active_workspace_id,
                ..
            } = msg
                && active_workspace_id == second_workspace
            {
                found_b_broadcast = true;
                break;
            }
        } else {
            break;
        }
    }
    assert!(
        found_b_broadcast,
        "另一个订阅者应当收到广播的 Workspaces 事件并感知活动工作区切换"
    );

    // 在 first_session（关联 first_workspace）中提交 prompt，验证 first_workspace 使用序号被推进
    engine.dispatch_to(
        ClientMessage::Submit {
            client_msg_id: "prompt-1".into(),
            session_id: first_session,
            prompt: "hello".into(),
        },
        Some(&sink_a),
        &mut |_| {},
    );

    let list_after_submit = engine.handle(ClientMessage::ListWorkspaces {
        client_msg_id: "check-sequence".into(),
    });
    if let [ServerMessage::Workspaces { workspaces, .. }] = list_after_submit.as_slice() {
        let ws1 = workspaces.iter().find(|w| w.id == first_workspace).unwrap();
        assert!(
            ws1.last_used_sequence > 0,
            "执行提问后关联工作区的 last_used_sequence 应当被推进"
        );
    } else {
        panic!("unexpected list response");
    }
}
