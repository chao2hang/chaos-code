//! 终端里聊过的对话，在这个主机上必须看得见、分得出在哪个目录聊的、并且能接着聊。
//!
//! 官方界面侧栏的分组靠每一行自己的 `workspacePath`，排序靠每一行自己的时刻。之前
//! 这两样都不存在：引擎只知道自己进程里建过的对话，界面只能把「服务启动时所在的目录」
//! 和「刚刚」贴到每一行上——于是终端在别的目录里聊的东西整个不见了，看见的几行还都
//! 顶着同一个目录、同一个时间。
//!
//! 这里钉住的是三件事：
//! 1. 存储镜像默认就在，列出来的每一行带着它自己的目录和它自己的时刻；
//! 2. 反复扫不会重复，终端那边改了标题/时长会跟上新来的一行，用户在这里改的不动；
//! 3. 接着聊走终端自己的 `--resume`，在原目录里跑，答案回到终端自己的会话文件。

use chaos_engine::{
    ClientMessage, Engine, PromptAdapter, RunOptions, ServerMessage, TimelineMessage,
};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const FIRST: &str = "01a11392-1bd4-7f82-ab78-df12c9f1028c";
const SECOND: &str = "01b22403-2ce5-8a93-bc89-e023dae2139d";

/// 造一份 CLI 真正写出来的样子的存储：`sessions/<urlencode(cwd)>/<uuid>/{summary.json,
/// chat_history.jsonl}`。
fn write_session(
    store: &Path,
    encoded: &str,
    id: &str,
    summary: serde_json::Value,
    turns: &[&str],
) {
    let dir = store.join(encoded).join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("summary.json"), summary.to_string()).unwrap();
    let mut history = String::new();
    for turn in turns {
        history.push_str(
            &serde_json::json!({
                "type": "user",
                "content": [{"type": "text", "text": format!("<user_query>{turn}</user_query>")}]
            })
            .to_string(),
        );
        history.push('\n');
        history.push_str(
            &serde_json::json!({"type": "assistant", "content": format!("关于「{turn}」的答复")})
                .to_string(),
        );
        history.push('\n');
    }
    fs::write(dir.join("chat_history.jsonl"), history).unwrap();
}

/// 两个目录，各一段对话——正是「在哪个目录聊的」这件事最需要被看见的形状。
fn fixture(store: &Path) {
    write_session(
        store,
        "%2Fhome%2Fdev%2Fweb-service",
        FIRST,
        serde_json::json!({
            "info": {"id": FIRST, "cwd": "/home/dev/web-service"},
            "generated_title": "3081 端口挂了",
            "created_at": "2026-10-08T01:00:00.000000Z",
            "last_active_at": "2026-10-08T02:00:00.000000Z",
            "num_chat_messages": 4
        }),
        &["为什么端口不通", "重启一下服务"],
    );
    write_session(
        store,
        "%2Fhome%2Fdev%2Fbackup-audit",
        SECOND,
        serde_json::json!({
            "info": {"id": SECOND, "cwd": "/home/dev/backup-audit"},
            "session_summary": "",
            "created_at": "2026-10-09T01:00:00.000000Z",
            "last_active_at": "2026-10-09T03:30:00.000000Z",
            "num_chat_messages": 2
        }),
        &["备份还活着吗"],
    );
}

fn sessions(engine: &Engine) -> Vec<chaos_engine::SessionInfo> {
    engine
        .handle(ClientMessage::ListSessions {
            client_msg_id: format!("list-{}", uuid::Uuid::new_v4()),
            workspace_id: None,
        })
        .into_iter()
        .find_map(|event| match event {
            ServerMessage::Sessions { sessions, .. } => Some(sessions),
            _ => None,
        })
        .unwrap_or_default()
}

/// 记下每一次运行被要求了什么——「继续对话到底以什么方式继续」只能从这里看。
#[derive(Clone, Default)]
struct RecordsRuns {
    runs: Arc<Mutex<Vec<RunOptions>>>,
}

impl PromptAdapter for RecordsRuns {
    fn run_prompt(&self, _prompt: &str, options: &RunOptions) -> Result<Vec<String>, String> {
        self.runs.lock().unwrap().push(options.clone());
        Ok(vec!["好的。".into()])
    }
}

#[test]
fn every_directorys_conversations_are_listed_with_their_own_directory() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let engine = Engine::new().with_chaos_store_root(store.path());
    let report = engine.import_chaos_sessions(store.path());
    assert_eq!(report.scanned, 2, "{report:?}");
    assert_eq!(report.imported, 2);
    assert_eq!(report.workspaces, 2);
    assert!(report.persistence_error.is_none(), "{report:?}");

    let listed = sessions(&engine);
    assert_eq!(listed.len(), 2, "两段对话都要在：{listed:?}");
    let by_cwd: Vec<(&str, Option<&str>)> = listed
        .iter()
        .map(|s| (s.title.as_str(), s.workspace_path.as_deref()))
        .collect();
    assert!(
        by_cwd.contains(&("3081 端口挂了", Some("/home/dev/web-service"))),
        "终端里有标题的用终端的标题：{by_cwd:?}"
    );
    assert!(
        by_cwd.contains(&("备份还活着吗", Some("/home/dev/backup-audit"))),
        "没有标题的退回第一句用户话，并且带着它自己的目录：{by_cwd:?}"
    );

    // 时间是对话自己的，不是「刚刚」。2026-10-08/09 与此刻相差以月计。
    let web = listed
        .iter()
        .find(|s| s.workspace_path.as_deref() == Some("/home/dev/web-service"))
        .unwrap();
    assert!(web.created_at_ms > 0 && web.updated_at_ms > web.created_at_ms);
    assert!(
        web.updated_at_ms < 2_000_000_000_000,
        "这是 2026 年 10 月的一次对话，不该被盖上重启时刻的时间：{}",
        web.updated_at_ms
    );
    // 列表按最后一次活跃排序：backup-audit（10-09）在 web-service（10-08）之前。
    assert_eq!(
        listed.first().unwrap().workspace_path.as_deref(),
        Some("/home/dev/backup-audit")
    );

    // 工作区列表也带着目录：侧栏要能在这些目录里开新对话。
    let workspaces = engine
        .handle(ClientMessage::ListWorkspaces {
            client_msg_id: "ws".into(),
        })
        .into_iter()
        .find_map(|event| match event {
            ServerMessage::Workspaces { workspaces, .. } => Some(workspaces),
            _ => None,
        })
        .unwrap_or_default();
    let paths: Vec<Option<&str>> = workspaces.iter().map(|w| w.path.as_deref()).collect();
    assert_eq!(paths.len(), 2, "{workspaces:?}");
    assert!(paths.contains(&Some("/home/dev/web-service")), "{paths:?}");
    assert!(paths.contains(&Some("/home/dev/backup-audit")));
}

/// 镜像不是档案。终端不再列的一段（删了、藏了、或按终端的规矩本来就不该列——
/// 比如没名字的空壳），界面上也得跟着消失，否则侧栏里全是它的客户端自己已经忘掉的东西。
#[test]
fn a_conversation_the_terminal_stopped_listing_leaves_the_listing() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let engine = Engine::new().with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());
    assert_eq!(sessions(&engine).len(), 2);

    // 终端那边把这段删掉了（等价地：藏起来、或清成终端不再列的形状）。
    fs::remove_dir_all(
        store
            .path()
            .join("%2Fhome%2Fdev%2Fbackup-audit")
            .join(SECOND),
    )
    .unwrap();
    let report = engine.import_chaos_sessions(store.path());
    assert_eq!(report.retired, 1, "终端不再列的，界面也要撤：{report:?}");
    let left = sessions(&engine);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].title, "3081 端口挂了", "{left:?}");
}

/// 撤的是镜像来的空行。已经在这个界面里聊过几句的会话留着——那份转写里有终端文件
/// 里没有的一轮，撤掉等于把用户刚拿到的回答扔掉。
#[test]
fn a_conversation_written_into_here_is_never_retired() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let recorder = RecordsRuns::default();
    let engine = Engine::with_adapter(recorder).with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());
    let audit = sessions(&engine)
        .into_iter()
        .find(|s| {
            s.workspace_path
                .as_deref()
                .is_some_and(|p| p.ends_with("backup-audit"))
        })
        .unwrap();
    engine.handle(ClientMessage::Submit {
        client_msg_id: "ask".into(),
        session_id: audit.session_id,
        prompt: "备份还活着吗".into(),
    });

    fs::remove_dir_all(store.path().join("%2Fhome%2Fdev%2Fbackup-audit")).unwrap();
    let report = engine.import_chaos_sessions(store.path());
    assert_eq!(report.retired, 0, "这里有内容的行不许撤：{report:?}");
    assert!(
        sessions(&engine)
            .iter()
            .any(|s| s.session_id == audit.session_id),
        "{:?}",
        sessions(&engine)
    );
}

#[test]
fn rescanning_never_duplicates_and_only_follows_the_terminals_edits() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let engine = Engine::new().with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());
    let first_ids: Vec<_> = sessions(&engine).iter().map(|s| s.session_id).collect();

    // 第二次扫：同一个对话还是同一行（id 由目录 + 终端会话 id 决定），什么都不新增。
    let again = engine.import_chaos_sessions(store.path());
    assert_eq!(again.scanned, 2);
    assert_eq!(again.imported, 0, "重复扫不该多出对话：{again:?}");
    assert_eq!(again.refreshed, 0);
    let after: Vec<_> = sessions(&engine).iter().map(|s| s.session_id).collect();
    assert_eq!(first_ids, after);

    // 终端那边自己改了标题：这里跟着改。
    let summary = store
        .path()
        .join("%2Fhome%2Fdev%2Fweb-service")
        .join(FIRST)
        .join("summary.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary).unwrap()).unwrap();
    value["generated_title"] = serde_json::json!("3081 端口挂了（已修好）");
    fs::write(&summary, value.to_string()).unwrap();
    let refreshed = engine.import_chaos_sessions(store.path());
    assert_eq!(refreshed.refreshed, 1, "标题变了要跟上：{refreshed:?}");
    assert!(
        sessions(&engine)
            .iter()
            .any(|s| s.title == "3081 端口挂了（已修好）"),
        "{:?}",
        sessions(&engine)
    );

    // 这里改名的，下一次扫不许被终端的标题盖掉。
    let web = sessions(&engine)
        .into_iter()
        .find(|s| s.workspace_path.as_deref() == Some("/home/dev/web-service"))
        .unwrap();
    engine.handle(ClientMessage::RenameSession {
        client_msg_id: "rename".into(),
        session_id: web.session_id,
        title: "我自己起的名字".into(),
    });
    engine.import_chaos_sessions(store.path());
    assert!(
        sessions(&engine)
            .iter()
            .any(|s| s.title == "我自己起的名字"),
        "用户的改名保住了吗：{:?}",
        sessions(&engine)
    );
}

#[test]
fn a_deleted_conversation_stays_deleted_across_rescans() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let engine = Engine::new().with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());
    let target = sessions(&engine)[0].session_id;
    engine.handle(ClientMessage::DeleteSession {
        client_msg_id: "delete".into(),
        session_id: target,
    });
    assert_eq!(sessions(&engine).len(), 1);
    engine.import_chaos_sessions(store.path());
    assert_eq!(
        sessions(&engine).len(),
        1,
        "终端的文件还在，但这里的删除要记住，否则列一遍就复活一次"
    );
}

#[test]
fn opening_one_reads_its_own_transcript() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let engine = Engine::new().with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());
    let listed = sessions(&engine);
    let audit = listed
        .iter()
        .find(|s| s.workspace_path.as_deref() == Some("/home/dev/backup-audit"))
        .unwrap();

    let events = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume".into(),
        session_id: audit.session_id,
        workspace_id: None,
    });
    let messages = events
        .into_iter()
        .find_map(|event| match event {
            ServerMessage::SessionSnapshot { messages, .. } => Some(messages),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(
        messages,
        vec![
            TimelineMessage {
                role: "user".into(),
                text: "备份还活着吗".into()
            },
            TimelineMessage {
                role: "assistant".into(),
                text: "关于「备份还活着吗」的答复".into()
            },
        ],
        "打开侧栏里的一行，看到的就是终端里那段对话本身"
    );
}

#[test]
fn continuing_one_resumes_the_terminals_own_session_in_its_own_directory() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let recorder = RecordsRuns::default();
    let engine = Engine::with_adapter(recorder.clone()).with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());

    let listed = sessions(&engine);
    let web = listed
        .iter()
        .find(|s| s.workspace_path.as_deref() == Some("/home/dev/web-service"))
        .unwrap();
    // 先看一眼（终端的转写被读进来），再问下一句。
    engine.handle(ClientMessage::Resume {
        client_msg_id: "resume".into(),
        session_id: web.session_id,
        workspace_id: None,
    });
    let events = engine.handle(ClientMessage::Submit {
        client_msg_id: "ask".into(),
        session_id: web.session_id,
        prompt: "现在端口通了吗".into(),
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ServerMessage::Completed { .. })),
        "{events:?}"
    );

    let runs = recorder.runs.lock().unwrap().clone();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(
        run.resume_session.as_deref(),
        Some(FIRST),
        "继续必须是让终端按它自己的会话 id 恢复，不是在界面上重放一遍文本"
    );
    assert_eq!(
        run.working_dir.as_deref(),
        Some(Path::new("/home/dev/web-service")),
        "问的是那个目录里的事，就得在那个目录里答"
    );
    // 有 `--resume` 的宿主自己带着上下文，历史不需要重发；但引擎这边仍然算好历史，
    // 好让没有 CLI 的宿主（HTTP provider）也能带上。
    assert!(
        run.history
            .iter()
            .any(|m| m.text == "为什么端口不通" || m.text == "重启一下服务"),
        "{:?}",
        run.history
    );
    assert!(
        !run.history
            .iter()
            .any(|m| m.role == "user" && m.text == "现在端口通了吗"),
        "正在问的这句由 prompt 传，不该在历史里再问一遍：{:?}",
        run.history
    );
}

#[test]
fn a_fresh_conversation_is_answered_in_its_own_workspace_not_resumed() {
    let store = tempfile::tempdir().unwrap();
    fixture(store.path());
    let recorder = RecordsRuns::default();
    let engine = Engine::with_adapter(recorder.clone()).with_chaos_store_root(store.path());
    engine.import_chaos_sessions(store.path());

    let workspace = engine.workspace_for_directory("/home/dev/web-service");
    let created = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "new".into(),
        workspace_id: Some(workspace),
    });
    let session_id = created
        .into_iter()
        .find_map(|event| match event {
            ServerMessage::SessionCreated { session_id, .. } => Some(session_id),
            _ => None,
        })
        .expect("CreateSession 要回一个 session id");
    // 新对话落在它自己的目录那一组里，而不是「默认工作区」。
    let listed = sessions(&engine);
    let row = listed
        .iter()
        .find(|s| s.session_id == session_id)
        .expect("刚建的对话要在列表里");
    assert_eq!(
        row.workspace_path.as_deref(),
        Some("/home/dev/web-service"),
        "{listed:?}"
    );

    engine.handle(ClientMessage::Submit {
        client_msg_id: "ask".into(),
        session_id,
        prompt: "看看这个目录".into(),
    });
    let runs = recorder.runs.lock().unwrap().clone();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].resume_session, None,
        "这里新建的对话没有终端会话可恢复"
    );
    assert_eq!(
        runs[0].working_dir.as_deref(),
        Some(Path::new("/home/dev/web-service"))
    );
    assert!(
        runs[0].history.is_empty(),
        "第一句之前没有历史：{:?}",
        runs[0].history
    );
}
