use super::*;
use crate::session::storage::jsonl::AppendDurability;

struct ActorGuard {
    handle: PersistenceHandle,
    task: tokio::task::JoinHandle<()>,
}

impl ActorGuard {
    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

fn test_actor(info: Info, storage: Arc<dyn StorageAdapter>) -> ActorGuard {
    test_actor_with_remote_sync(info, storage, None)
}

fn test_actor_with_remote_sync(
    info: Info,
    storage: Arc<dyn StorageAdapter>,
    remote_sync: Option<RemoteSync>,
) -> ActorGuard {
    test_actor_inner(info, storage, remote_sync, false)
}

fn test_actor_inner(
    info: Info,
    storage: Arc<dyn StorageAdapter>,
    remote_sync: Option<RemoteSync>,
    mark_summary_done: bool,
) -> ActorGuard {
    test_actor_with_sampler(
        info,
        storage,
        remote_sync,
        mark_summary_done,
        xai_grok_sampler::SamplerConfig::default(),
    )
}

/// `sampler_config` decides where the title generator sends its request.
/// The default config has an empty `base_url`, so `reqwest` fails to build the request and no
/// socket is ever opened; the title falls back to truncated user text without leaving the
/// process. A test that needs the generation to be observably still running has to point this
/// at an endpoint of its own.
fn test_actor_with_sampler(
    info: Info,
    storage: Arc<dyn StorageAdapter>,
    remote_sync: Option<RemoteSync>,
    mark_summary_done: bool,
    sampler_config: xai_grok_sampler::SamplerConfig,
) -> ActorGuard {
    let (tx, rx) = mpsc::unbounded_channel();
    let (disk_full_tx, disk_full_rx) = tokio::sync::watch::channel(false);
    let model = sampler_config.model.clone();
    let sampling_client = OaiCompatClient::new(sampler_config).unwrap();
    let mut summary =
        crate::session::summary::SummaryGenerator::new(crate::session::summary::SummaryConfig {
            sampling_client,
            model,
            persistence_tx: tx.downgrade(),
        });
    if mark_summary_done {
        summary.mark_done();
    }
    let task = tokio::spawn(
        SessionPersistence {
            info,
            storage,
            pending_notification: None,
            rx,
            remote_sync,
            // These tests run the actor as resumed; the backfill on writeback upgrade only runs for a fresh session
            created_fresh: false,
            relay_sync: None,
            summary,
            registry_title_sync: None,
            gateway: None,
            search_index: crate::session::storage::search::SharedSearchIndex::never_indexed(),
            disk_full_tx,
            disk_full_notified: false,
            dirty_files: Default::default(),
            pending_write_error: None,
            last_usage_live: None,
            last_usage_turn: None,
            last_incoming_turn: None,
            pending_auto_title: None,
        }
        .run(),
    );
    ActorGuard {
        handle: PersistenceHandle::from_parts_for_test(tx, disk_full_rx),
        task,
    }
}

fn notification(info: &Info, text: &str) -> acp::SessionNotification {
    acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(text),
        ))),
    )
}

fn neutral_update(info: &Info, text: &str) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(notification(info, text)))
}

#[tokio::test]
async fn writeback_backfill_is_fresh_only_and_acp_only() {
    let info = Info {
        id: acp::SessionId::new("wb-backfill"),
        cwd: "/test".into(),
    };

    // Fresh session: every ACP update is queued to the writeback sync.
    let (sync, mut observed) = RemoteSync::test_observer();
    let updates = vec![neutral_update(&info, "a"), neutral_update(&info, "b")];
    let n = backfill_updates_to_sync(true, updates, &sync);
    assert_eq!(n, 2, "a fresh session backfills its full local ACP history");
    for _ in 0..2 {
        tokio::time::timeout(std::time::Duration::from_secs(1), observed.recv())
            .await
            .expect("backfilled notification not observed within 1s")
            .expect("observer channel closed unexpectedly");
    }

    // Resumed session: nothing is backfilled (prior history may already be synced).
    let (sync2, mut observed2) = RemoteSync::test_observer();
    let n2 = backfill_updates_to_sync(false, vec![neutral_update(&info, "a")], &sync2);
    assert_eq!(n2, 0, "a resumed session is forward-only, no backfill");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(200), observed2.recv())
            .await
            .is_err(),
        "resumed session must not re-send any prior history",
    );
}

fn break_summary_writes(dir: &std::path::Path) {
    let summary = dir.join("summary.json");
    std::fs::remove_file(&summary).unwrap();
    std::fs::create_dir(summary).unwrap();
}

fn break_plan_writes(dir: &std::path::Path) {
    std::fs::create_dir(dir.join("plan.json")).unwrap();
}

async fn recv_observed(
    observed: &mut tokio::sync::mpsc::UnboundedReceiver<acp::SessionNotification>,
) -> acp::SessionNotification {
    tokio::time::timeout(std::time::Duration::from_secs(1), observed.recv())
        .await
        .expect("remote sync timed out")
        .expect("remote sync observer closed")
}

#[test]
fn committed_error_returns_sync_disposition() {
    let info = Info {
        id: acp::SessionId::new("committed-update"),
        cwd: "/test".into(),
    };
    let notification = notification(&info, "committed");
    let PendingAppendOutcome::CommittedErr(sync_notification, error) =
        SessionPersistence::finish_pending_append(
            notification,
            Err(crate::session::storage::AppendUpdateError::Committed(
                io::Error::other("summary patch failed"),
            )),
        )
    else {
        panic!("expected committed failure");
    };
    assert_eq!(sync_notification.session_id, info.id);
    assert_eq!(error.to_string(), "summary patch failed");
}

#[test]
fn uncommitted_error_returns_restore_disposition() {
    let info = Info {
        id: acp::SessionId::new("uncommitted-update"),
        cwd: "/test".into(),
    };
    let notification = notification(&info, "pending");
    let PendingAppendOutcome::NotCommittedErr(pending_notification, error) =
        SessionPersistence::finish_pending_append(
            notification,
            Err(crate::session::storage::AppendUpdateError::NotCommitted(
                io::Error::other("append failed"),
            )),
        )
    else {
        panic!("expected uncommitted failure");
    };
    assert_eq!(pending_notification.session_id, info.id);
    assert_eq!(error.to_string(), "append failed");
}

#[tokio::test]
async fn noop_handle_rejects_durable_append() {
    let info = Info {
        id: acp::SessionId::new("noop-durable-update"),
        cwd: "/test".into(),
    };
    assert!(matches!(
        PersistenceHandle::noop()
            .append_update_durably(neutral_update(&info, "durable"))
            .await,
        Err(DurableAppendError::NotCommitted(error))
            if error.kind() == io::ErrorKind::Unsupported
    ));
}

#[tokio::test]
async fn pending_drain_disposition_controls_remote_sync() {
    let info = Info {
        id: acp::SessionId::new("pending-remote-sync"),
        cwd: "/test".into(),
    };
    let storage = JsonlStorageAdapter::with_update_append_probe("/unused".into(), |_| {
        Err(io::Error::other("append failed"))
    });
    let (remote_sync, mut observed) = RemoteSync::test_observer();
    let actor = test_actor_with_remote_sync(info.clone(), Arc::new(storage), Some(remote_sync));
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    assert!(matches!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "durable"))
            .await,
        Err(DurableAppendError::NotCommitted(_))
    ));
    assert!(observed.try_recv().is_err());
    actor.stop().await;

    let dir = tempfile::tempdir().unwrap();
    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_attempts = attempts.clone();
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().to_path_buf(),
        move |durability| {
            observed_attempts.lock().unwrap().push(durability);
            Ok(())
        },
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let (remote_sync, mut observed) = RemoteSync::test_observer();
    let actor = test_actor_with_remote_sync(info.clone(), storage, Some(remote_sync));
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    break_summary_writes(dir.path());
    assert!(matches!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "durable"))
            .await,
        Err(DurableAppendError::Committed(_))
    ));
    let synced = recv_observed(&mut observed).await;
    assert_eq!(synced.session_id, info.id);
    assert!(matches!(
        attempts.lock().unwrap().as_slice(),
        [AppendDurability::Buffered, AppendDurability::Durable]
    ));
    actor.stop().await;
}

#[tokio::test]
async fn durable_append_committed_failure_is_synced() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("durable-remote-sync"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    break_summary_writes(dir.path());
    let (remote_sync, mut observed) = RemoteSync::test_observer();
    let actor = test_actor_with_remote_sync(info.clone(), storage, Some(remote_sync));
    assert!(matches!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "durable"))
            .await,
        Err(DurableAppendError::Committed(_))
    ));
    let synced = recv_observed(&mut observed).await;
    assert_eq!(synced.session_id, info.id);
    actor.stop().await;
}

#[tokio::test]
async fn failed_pending_drain_retains_record_and_skips_durable_update() {
    let info = Info {
        id: acp::SessionId::new("durable-drain-failure"),
        cwd: "/test".into(),
    };
    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = attempts.clone();
    let storage =
        JsonlStorageAdapter::with_update_append_probe("/unused".into(), move |durability| {
            observed.lock().unwrap().push(durability);
            Err(io::Error::other("pending append failed"))
        });
    let actor = test_actor(info.clone(), Arc::new(storage));
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            actor
                .handle
                .append_update_durably(neutral_update(&info, "durable"))
                .await
                .unwrap_err()
                .to_string(),
            "pending append failed"
        );
    }
    assert!(matches!(
        attempts.lock().unwrap().as_slice(),
        [AppendDurability::Buffered, AppendDurability::Buffered]
    ));
    actor.stop().await;
}

#[tokio::test]
async fn committed_pending_drain_still_writes_the_durable_update() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("durable-after-committed-drain"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    break_summary_writes(dir.path());
    assert!(matches!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "terminal"))
            .await,
        Err(DurableAppendError::Committed(_))
    ));
    let jsonl = std::fs::read_to_string(dir.path().join("updates.jsonl")).unwrap();
    assert!(
        jsonl.contains("pending") && jsonl.contains("terminal"),
        "a committed drain must not drop the durable terminal: {jsonl}"
    );
    actor.stop().await;
}

#[tokio::test]
async fn durable_append_drains_pending_update_in_fifo_order() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("durable-update"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage.clone());
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "before")))
        .unwrap();
    actor
        .handle
        .append_update_durably(neutral_update(&info, "durable"))
        .await
        .unwrap();
    let summary = storage.load_summary(&info).await.unwrap();
    assert_eq!(summary.num_messages, 2);

    let updates = storage.load_session(&info).await.unwrap().updates;
    let texts = updates
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Acp(notification) = update else {
                return None;
            };
            let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                return None;
            };
            let acp::ContentBlock::Text(text) = &chunk.content else {
                return None;
            };
            Some(text.text.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["before", "durable"]);
    actor.stop().await;
}

async fn flush_ack(handle: &PersistenceHandle) -> io::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .tx
        .send(PersistenceMsg::FlushAndAck { respond_to: tx })
        .unwrap();
    rx.await.unwrap()
}

fn merge_boundary_update(info: &Info, text: &str) -> SessionUpdate {
    let mut chunk_meta = serde_json::Map::new();
    chunk_meta.insert("mergeBoundary".into(), serde_json::json!(true));
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::AgentMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
                .meta(Some(chunk_meta)),
        ),
    )))
}

struct SyncBarrierProbe {
    appends: Arc<std::sync::Mutex<Vec<AppendDurability>>>,
    syncs: Arc<std::sync::Mutex<Vec<crate::session::storage::SessionFileSet>>>,
}

fn actor_with_barrier_probes(dir: &std::path::Path, info: &Info) -> (ActorGuard, SyncBarrierProbe) {
    let appends = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_appends = appends.clone();
    let syncs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_syncs = syncs.clone();
    let storage = Arc::new(JsonlStorageAdapter::with_probes(
        dir.to_path_buf(),
        move |durability| {
            observed_appends.lock().unwrap().push(durability);
            Ok(())
        },
        move |files| {
            observed_syncs.lock().unwrap().push(files);
            Ok(())
        },
    ));
    let actor = test_actor(info.clone(), storage);
    (actor, SyncBarrierProbe { appends, syncs })
}

#[tokio::test]
async fn flush_and_ack_syncs_only_dirty_files_once_and_keeps_streamed_appends_buffered() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-sync"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    for text in ["chunk-a", "chunk-b"] {
        actor
            .handle
            .tx
            .send(PersistenceMsg::Update(neutral_update(&info, text)))
            .unwrap();
    }
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(merge_boundary_update(
            &info, "boundary",
        )))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    assert!(
        matches!(
            probe.appends.lock().unwrap().as_slice(),
            [AppendDurability::Buffered, AppendDurability::Buffered]
        ),
        "every streamed-chunk append must stay buffered (no per-chunk syncs)"
    );
    assert_eq!(
        probe.syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            updates: true,
            ..Default::default()
        }],
        "the barrier must sync exactly once, covering only the dirtied updates file"
    );

    flush_ack(&actor.handle).await.unwrap();
    assert_eq!(
        probe.syncs.lock().unwrap().len(),
        1,
        "a second barrier with nothing dirtied since the first must sync no files"
    );
    actor.stop().await;
}

#[tokio::test]
async fn idle_flush_and_ack_acks_without_syncing_any_files() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-idle"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    flush_ack(&actor.handle).await.unwrap();

    assert!(
        probe.syncs.lock().unwrap().is_empty(),
        "an idle barrier must not pay for untouched files"
    );
    actor.stop().await;
}

#[tokio::test]
async fn buffered_chat_plan_and_rewind_writes_dirty_exactly_their_files() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-dirty-set"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Chat(ConversationItem::user("hello")))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::PlanState(TodoState::default()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::RewindPoint(RewindPoint::new(0)))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        probe.syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            chat: true,
            rewind_points: true,
            ..Default::default()
        }],
        "buffered chat/rewind writes must dirty exactly their files; \
         atomic-rename writes (plan, summary bookkeeping) are durable at write time and stay out"
    );
    actor.stop().await;
}

#[tokio::test]
async fn copy_file_flush_syncs_every_file_regardless_of_dirtiness() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("copy-file-sync-all"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    let (one_shot, copied) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::CopyFile { one_shot })
        .unwrap();
    copied.await.unwrap().unwrap();

    assert_eq!(
        probe.syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet::ALL],
        "a CopyFile snapshot must sync the full barrier file set even when clean"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_syncs_chat_when_summary_bookkeeping_fails_after_append() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-chat-committed"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    break_summary_writes(dir.path());
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Chat(ConversationItem::user("hello")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        probe.syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            chat: true,
            ..Default::default()
        }],
        "a chat append that reached the page cache must stay on the barrier dirty set even when summary bookkeeping fails"
    );
    assert!(
        std::fs::read_to_string(dir.path().join("chat_history.jsonl"))
            .unwrap()
            .contains("hello"),
        "the chat JSONL record must survive the bookkeeping failure"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_succeeds_after_committed_pending_drain() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-committed-drain"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    break_summary_writes(dir.path());
    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        probe.syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            updates: true,
            ..Default::default()
        }],
        "a Committed pending drain must not fail FlushAndAck after the prompt bytes are on the dirty set"
    );
    assert!(
        std::fs::read_to_string(dir.path().join("updates.jsonl"))
            .unwrap()
            .contains("pending"),
        "the pending JSONL record must survive the bookkeeping failure"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_succeeds_after_not_committed_pending_drain() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-restored-drain"),
        cwd: "/test".into(),
    };
    let remaining_failures = std::sync::atomic::AtomicUsize::new(1);
    let syncs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_syncs = syncs.clone();
    let storage = Arc::new(JsonlStorageAdapter::with_probes(
        dir.path().to_path_buf(),
        move |durability| {
            if matches!(durability, AppendDurability::Buffered)
                && remaining_failures.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1
            {
                Err(io::Error::other("pending drain failed"))
            } else {
                Ok(())
            }
        },
        move |files| {
            observed_syncs.lock().unwrap().push(files);
            Ok(())
        },
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    assert_eq!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "terminal"))
            .await
            .unwrap_err()
            .to_string(),
        "pending drain failed"
    );

    actor
        .handle
        .tx
        .send(PersistenceMsg::Chat(ConversationItem::user("hello")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            chat: true,
            updates: true,
            ..Default::default()
        }],
        "a restored NotCommitted drain must not latch into the prompt barrier after a later successful redrain and chat append"
    );
    assert!(
        std::fs::read_to_string(dir.path().join("updates.jsonl"))
            .unwrap()
            .contains("pending"),
        "the restored pending record must be written on the next FlushAndAck"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_succeeds_after_atomic_plan_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-plan-fail"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    break_plan_writes(dir.path());
    let (actor, probe) = actor_with_barrier_probes(dir.path(), &info);

    actor
        .handle
        .tx
        .send(PersistenceMsg::PlanState(TodoState::default()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Chat(ConversationItem::user("hello")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        probe.syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            chat: true,
            ..Default::default()
        }],
        "a failed atomic-rename plan write must not latch into the prompt barrier or skip a later successful chat append"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_succeeds_after_durable_append_never_reached_disk() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-durable-not-committed"),
        cwd: "/test".into(),
    };
    let syncs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_syncs = syncs.clone();
    let storage = Arc::new(JsonlStorageAdapter::with_probes(
        dir.path().to_path_buf(),
        |durability| match durability {
            AppendDurability::Durable => Err(io::Error::other("durable append failed")),
            AppendDurability::Buffered => Ok(()),
        },
        move |files| {
            observed_syncs.lock().unwrap().push(files);
            Ok(())
        },
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    assert_eq!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "terminal"))
            .await
            .unwrap_err()
            .to_string(),
        "durable append failed"
    );

    actor
        .handle
        .tx
        .send(PersistenceMsg::Chat(ConversationItem::user("hello")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            chat: true,
            ..Default::default()
        }],
        "a NotCommitted durable append must not latch into the prompt barrier or skip a later successful chat append"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_retries_fsync_after_durable_append_file_barrier_failure() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-durable-barrier"),
        cwd: "/test".into(),
    };
    JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf())
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let syncs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_syncs = syncs.clone();
    let storage = Arc::new(
        JsonlStorageAdapter::with_probes(
            dir.path().to_path_buf(),
            |_| Ok(()),
            move |files| {
                observed_syncs.lock().unwrap().push(files);
                Ok(())
            },
        )
        .with_file_sync_probe(|| Err(io::Error::other("file barrier failed"))),
    );
    let actor = test_actor(info.clone(), storage);

    assert!(matches!(
        actor
            .handle
            .append_update_durably(neutral_update(&info, "terminal"))
            .await,
        Err(DurableAppendError::Committed(_))
    ));
    assert!(
        std::fs::read_to_string(dir.path().join("updates.jsonl"))
            .unwrap()
            .contains("terminal"),
        "the durable JSONL record must survive the file-barrier failure"
    );

    flush_ack(&actor.handle).await.unwrap();

    assert_eq!(
        syncs.lock().unwrap().as_slice(),
        [crate::session::storage::SessionFileSet {
            updates: true,
            ..Default::default()
        }],
        "a durable append whose file barrier failed must stay on the dirty set so a later idle FlushAndAck retries the fsync"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_fails_after_copy_file_when_a_buffered_write_never_reached_disk() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-copy-file-latch"),
        cwd: "/test".into(),
    };
    let remaining_failures = std::sync::atomic::AtomicUsize::new(1);
    let storage = Arc::new(JsonlStorageAdapter::with_probes(
        dir.path().to_path_buf(),
        move |_| {
            if remaining_failures.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                Err(io::Error::other("update append failed"))
            } else {
                Ok(())
            }
        },
        |_| Ok(()),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "chunk")))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(merge_boundary_update(
            &info, "boundary",
        )))
        .unwrap();

    let (one_shot, copied) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::CopyFile { one_shot })
        .unwrap();
    copied.await.unwrap().unwrap();

    assert_eq!(
        flush_ack(&actor.handle).await.unwrap_err().to_string(),
        "update append failed",
        "CopyFile must not take the write-failure latch; FlushAndAck still withholds persist_ack"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_fails_when_a_buffered_update_write_never_reached_disk() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-lost-write"),
        cwd: "/test".into(),
    };
    let remaining_failures = std::sync::atomic::AtomicUsize::new(1);
    let storage = Arc::new(JsonlStorageAdapter::with_probes(
        dir.path().to_path_buf(),
        move |_| {
            if remaining_failures.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                Err(io::Error::other("update append failed"))
            } else {
                Ok(())
            }
        },
        |_| Ok(()),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "chunk")))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(merge_boundary_update(
            &info, "boundary",
        )))
        .unwrap();
    assert_eq!(
        flush_ack(&actor.handle).await.unwrap_err().to_string(),
        "update append failed"
    );
    actor.stop().await;
}

#[tokio::test]
async fn flush_and_ack_propagates_session_file_sync_error_through_the_ack() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("flush-ack-sync-error"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_probes(
        dir.path().to_path_buf(),
        |_| Ok(()),
        |_| Err(io::Error::other("session file sync failed")),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "chunk")))
        .unwrap();
    assert_eq!(
        flush_ack(&actor.handle).await.unwrap_err().to_string(),
        "session file sync failed"
    );
    actor.stop().await;
}

/// Baselines on APFS (M-series laptop SSD), 50 iterations, medians:
/// prompt-send FlushAndAck round-trip ~26 ms (max ~48 ms);
/// idle FlushAndAck ~30-40 us with zero file syncs (was ~5 ms for the fixed 5-file set before dirty tracking);
/// barrier sync of 2 dirty files and their dir ~4-5 ms;
/// summary.json atomic rewrite (per-append bookkeeping) ~10 ms.
#[tokio::test]
#[ignore = "manual durability-cost measurement; run with --ignored --nocapture and RUST_MIN_STACK=8388608"]
async fn measure_prompt_barrier_idle_barrier_and_summary_rewrite_cost() {
    fn median_and_max(mut samples: Vec<std::time::Duration>) -> (String, String) {
        samples.sort();
        (
            format!("{:?}", samples[samples.len() / 2]),
            format!("{:?}", samples[samples.len() - 1]),
        )
    }

    const N: usize = 50;
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("measure-barrier"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage.clone());

    actor
        .handle
        .tx
        .send(PersistenceMsg::Chat(ConversationItem::user("seed")))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::PlanState(TodoState::default()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::RewindPoint(RewindPoint::new(0)))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let mut prompt_shaped_barrier = Vec::with_capacity(N);
    for index in 0..N {
        actor
            .handle
            .tx
            .send(PersistenceMsg::Chat(ConversationItem::user(format!(
                "prompt {index}"
            ))))
            .unwrap();
        actor
            .handle
            .tx
            .send(PersistenceMsg::Update(neutral_update(&info, "user echo")))
            .unwrap();
        let start = std::time::Instant::now();
        flush_ack(&actor.handle).await.unwrap();
        prompt_shaped_barrier.push(start.elapsed());
    }

    let mut idle_barrier = Vec::with_capacity(N);
    for _ in 0..N {
        let start = std::time::Instant::now();
        flush_ack(&actor.handle).await.unwrap();
        idle_barrier.push(start.elapsed());
    }

    let dirty_two = crate::session::storage::SessionFileSet {
        updates: true,
        chat: true,
        ..Default::default()
    };
    let mut sync_two_files = Vec::with_capacity(N);
    for _ in 0..N {
        let start = std::time::Instant::now();
        storage
            .sync_session_files_selected(&info, dirty_two)
            .await
            .unwrap();
        sync_two_files.push(start.elapsed());
    }

    let mut sync_all_files = Vec::with_capacity(N);
    for _ in 0..N {
        let start = std::time::Instant::now();
        storage
            .sync_session_files_selected(&info, crate::session::storage::SessionFileSet::ALL)
            .await
            .unwrap();
        sync_all_files.push(start.elapsed());
    }

    let summary_path = dir.path().join("summary.json");
    let payload = std::fs::read(&summary_path).unwrap();
    let mut summary_rewrite = Vec::with_capacity(N);
    for _ in 0..N {
        let start = std::time::Instant::now();
        crate::session::storage::write_bytes_atomic(&summary_path, &payload).unwrap();
        summary_rewrite.push(start.elapsed());
    }

    for (label, samples) in [
        (
            "prompt-send FlushAndAck (chat+echo appends, their summary bookkeeping, dirty barrier)",
            prompt_shaped_barrier,
        ),
        ("idle FlushAndAck (nothing dirty)", idle_barrier),
        ("barrier sync of 2 dirty files + dir", sync_two_files),
        (
            "barrier sync of all 5 files + dir (pre-dirty-tracking shape)",
            sync_all_files,
        ),
        (
            "summary.json atomic rewrite (per-append bookkeeping cost)",
            summary_rewrite,
        ),
    ] {
        let (median, max) = median_and_max(samples);
        println!("{label}: median {median}, max {max}");
    }

    actor.stop().await;
}

async fn probe_writable(handle: &PersistenceHandle) -> io::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .tx
        .send(PersistenceMsg::ProbeWritable { respond_to: tx })
        .unwrap();
    rx.await.unwrap()
}

/// Seeds `RemoteSync` with a pre-rename title (the cache at init), drives `PersistenceMsg::ManualTitleRenamed`, then queues an update and flushes.
/// The flush's `save_session_data` payload must carry the manual title.
#[tokio::test]
async fn manual_rename_next_flush_does_not_revert_backend_title() {
    use std::sync::Arc;

    use crate::auth::{AuthManager, GrokAuth};
    use crate::remote::BackendClient;
    use crate::session::export::ExportedMetadata;
    use xai_grok_test_support::MockInferenceServer;

    const OLD_TITLE: &str = "Auto first-prompt summary";
    const NEW_TITLE: &str = "Manual rename";
    const SESSION_ID: &str = "rename-writeback";

    let server = MockInferenceServer::start()
        .await
        .expect("start MockInferenceServer");
    let home = tempfile::tempdir().unwrap();
    let auth = Arc::new(AuthManager::new(
        home.path(),
        crate::auth::GrokComConfig::default(),
    ));
    auth.hot_swap(GrokAuth {
        key: "writeback-test-token".into(),
        ..GrokAuth::test_default()
    });

    let info = Info {
        id: acp::SessionId::new(SESSION_ID),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();

    let metadata = ExportedMetadata {
        title: Some(OLD_TITLE.into()),
        cwd: info.cwd.clone(),
        model_id: Some("test-model".into()),
        created_at: None,
        updated_at: None,
        total_messages: None,
        parent_session_id: None,
        session_kind: None,
        subagent_type: None,
        subagent_persona: None,
        subagent_role: None,
        fork_context_source: None,
        subagent_depth: None,
        title_is_manual: None,
    };
    let client = BackendClient::with_base_url(server.origin()).with_auth_manager(auth);
    let remote_sync = RemoteSync::new(SESSION_ID.to_owned(), metadata, client);
    let actor = test_actor_with_remote_sync(info.clone(), storage, Some(remote_sync));

    actor
        .handle
        .tx
        .send(PersistenceMsg::ManualTitleRenamed(NEW_TITLE.into()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(
            &info,
            "turn after rename",
        )))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let titles = wait_for_save_session_titles(&server, SESSION_ID).await;
    let last_nonempty = titles
        .iter()
        .rev()
        .find(|t| t.nonempty_messages)
        .map(|t| t.title.as_str());
    assert_eq!(
        last_nonempty,
        Some(NEW_TITLE),
        "next RemoteSync flush after ManualTitleRenamed must not revert to {OLD_TITLE:?}"
    );
    assert!(
        titles
            .iter()
            .filter(|t| t.nonempty_messages)
            .all(|t| t.title != OLD_TITLE),
        "no non-empty save_session_data may carry the pre-rename title"
    );
    assert!(
        titles
            .iter()
            .filter(|t| t.title == NEW_TITLE)
            .all(|t| t.title_is_manual == Some(true)),
        "every save of the manual title must stamp title_is_manual: {titles:?}"
    );
    let upserted_title =
        wait_for_upserted_title(&server, SESSION_ID, std::time::Duration::from_secs(5)).await;
    assert_eq!(
        upserted_title.as_deref(),
        Some(NEW_TITLE),
        "SetTitle must upsert the session-row title, not only the metadata blob; requests={:?}",
        request_path_summary(&server)
    );
    actor.stop().await;
}

/// Poll until a `PUT /sessions/{id}` upsert lands, or the deadline passes.
///
/// `RemoteSync` issues `save_session_data` (POST) *then* `upsert_session`
/// (PUT) as two sequential awaits: observing the POST does not imply the PUT
/// has reached the wire yet. Polling here removes that scheduling window
/// without weakening the assertion — on timeout the caller still asserts
/// against `None` and fails with the same diagnostics.
async fn wait_for_upserted_title(
    server: &xai_grok_test_support::MockInferenceServer,
    session_id: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    let upsert_path = format!("/sessions/{session_id}");
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let title = find_upserted_title(server, &upsert_path);
        if title.is_some() || tokio::time::Instant::now() >= deadline {
            return title;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn find_upserted_title(
    server: &xai_grok_test_support::MockInferenceServer,
    upsert_path: &str,
) -> Option<String> {
    server.requests().into_iter().rev().find_map(|r| {
        (r.method == "PUT" && r.path == upsert_path)
            .then(|| {
                r.body
                    .as_ref()?
                    .get("session")?
                    .get("title")?
                    .as_str()
                    .map(str::to_owned)
            })
            .flatten()
    })
}

#[derive(Debug)]
struct SaveTitle {
    title: String,
    title_present: bool,
    nonempty_messages: bool,
    title_is_manual: Option<bool>,
}

fn save_session_titles(
    server: &xai_grok_test_support::MockInferenceServer,
    session_id: &str,
) -> Vec<SaveTitle> {
    let path = format!("/sessions/{session_id}/data");
    server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST" && r.path == path)
        .filter_map(|r| {
            let body = r.body.as_ref()?;
            let metadata = body.get("metadata");
            let title_field = metadata.and_then(|m| m.get("title"));
            let title_present = title_field.is_some();
            let title = title_field
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_owned();
            let title_is_manual = metadata
                .and_then(|m| m.get("title_is_manual"))
                .and_then(|v| v.as_bool());
            let nonempty_messages = body
                .get("messages")
                .and_then(|m| m.as_array())
                .is_some_and(|msgs| !msgs.is_empty());
            Some(SaveTitle {
                title,
                title_present,
                nonempty_messages,
                title_is_manual,
            })
        })
        .collect()
}

fn request_path_summary(server: &xai_grok_test_support::MockInferenceServer) -> Vec<String> {
    server
        .requests()
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

async fn wait_for_save_session_titles(
    server: &xai_grok_test_support::MockInferenceServer,
    session_id: &str,
) -> Vec<SaveTitle> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let titles = save_session_titles(server, session_id);
        if titles.iter().any(|t| t.nonempty_messages) {
            return titles;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for flush save_session_data; requests={:?}",
                request_path_summary(server)
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn manual_after_auto_last_flush_is_manual() {
    use std::sync::Arc;

    use crate::auth::{AuthManager, GrokAuth};
    use crate::remote::BackendClient;
    use crate::session::export::ExportedMetadata;
    use xai_grok_test_support::MockInferenceServer;

    const AUTO: &str = "Auto title";
    const MANUAL: &str = "Manual wins";
    const SESSION_ID: &str = "rename-fifo-auto-then-manual";

    let server = MockInferenceServer::start()
        .await
        .expect("start MockInferenceServer");
    let home = tempfile::tempdir().unwrap();
    let auth = Arc::new(AuthManager::new(
        home.path(),
        crate::auth::GrokComConfig::default(),
    ));
    auth.hot_swap(GrokAuth {
        key: "writeback-test-token".into(),
        ..GrokAuth::test_default()
    });

    let info = Info {
        id: acp::SessionId::new(SESSION_ID),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();

    let metadata = ExportedMetadata {
        title: Some("pre".into()),
        cwd: info.cwd.clone(),
        model_id: Some("test-model".into()),
        created_at: None,
        updated_at: None,
        total_messages: None,
        parent_session_id: None,
        session_kind: None,
        subagent_type: None,
        subagent_persona: None,
        subagent_role: None,
        fork_context_source: None,
        subagent_depth: None,
        title_is_manual: None,
    };
    let client = BackendClient::with_base_url(server.origin()).with_auth_manager(auth);
    let remote_sync = RemoteSync::new(SESSION_ID.to_owned(), metadata, client);
    let actor = test_actor_with_remote_sync(info.clone(), storage, Some(remote_sync));

    actor
        .handle
        .tx
        .send(PersistenceMsg::GeneratedTitle(AUTO.into()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::ManualTitleRenamed(MANUAL.into()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "after")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let titles = wait_for_save_session_titles(&server, SESSION_ID).await;
    let last = titles
        .iter()
        .rev()
        .find(|t| t.nonempty_messages)
        .map(|t| t.title.as_str());
    assert_eq!(
        last,
        Some(MANUAL),
        "auto-then-manual last flush must be manual"
    );
    assert!(
        titles
            .iter()
            .filter(|t| t.title == AUTO)
            .all(|t| t.title_is_manual.is_none()),
        "auto SetTitle must omit title_is_manual: {titles:?}"
    );
    assert!(
        titles
            .iter()
            .filter(|t| t.title == MANUAL)
            .all(|t| t.title_is_manual == Some(true)),
        "manual SetTitle/flush must stamp title_is_manual: {titles:?}"
    );
    actor.stop().await;
}

#[tokio::test]
async fn auto_after_committed_manual_emits_no_set_title() {
    use std::sync::Arc;

    use crate::auth::{AuthManager, GrokAuth};
    use crate::remote::BackendClient;
    use crate::session::export::ExportedMetadata;
    use xai_grok_test_support::MockInferenceServer;

    const AUTO: &str = "Rejected auto";
    const MANUAL: &str = "Pinned manual";
    const SESSION_ID: &str = "rename-fifo-manual-then-auto";

    let server = MockInferenceServer::start()
        .await
        .expect("start MockInferenceServer");
    let home = tempfile::tempdir().unwrap();
    let auth = Arc::new(AuthManager::new(
        home.path(),
        crate::auth::GrokComConfig::default(),
    ));
    auth.hot_swap(GrokAuth {
        key: "writeback-test-token".into(),
        ..GrokAuth::test_default()
    });

    let info = Info {
        id: acp::SessionId::new(SESSION_ID),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .update_session_title(&info, MANUAL.to_owned())
        .await
        .unwrap();

    let metadata = ExportedMetadata {
        title: Some("stale".into()),
        cwd: info.cwd.clone(),
        model_id: Some("test-model".into()),
        created_at: None,
        updated_at: None,
        total_messages: None,
        parent_session_id: None,
        session_kind: None,
        subagent_type: None,
        subagent_persona: None,
        subagent_role: None,
        fork_context_source: None,
        subagent_depth: None,
        title_is_manual: None,
    };
    let client = BackendClient::with_base_url(server.origin()).with_auth_manager(auth);
    let remote_sync = RemoteSync::new(SESSION_ID.to_owned(), metadata, client);
    let actor = test_actor_with_remote_sync(info.clone(), storage, Some(remote_sync));

    actor
        .handle
        .tx
        .send(PersistenceMsg::ManualTitleRenamed(MANUAL.into()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::GeneratedTitle(AUTO.into()))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "after")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let titles = wait_for_save_session_titles(&server, SESSION_ID).await;
    assert!(
        titles.iter().all(|t| t.title != AUTO),
        "rejected auto title must not reach save_session_data"
    );
    let last = titles
        .iter()
        .rev()
        .find(|t| t.nonempty_messages)
        .map(|t| t.title.as_str());
    assert_eq!(last, Some(MANUAL));
    assert!(
        titles
            .iter()
            .filter(|t| t.title == MANUAL)
            .all(|t| t.title_is_manual == Some(true)),
        "manual stamp must survive rejected auto + flush: {titles:?}"
    );
    actor.stop().await;
}

#[tokio::test]
async fn manual_title_renamed_is_noop_without_remote_sync() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("rename-local-only"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(PersistenceMsg::ManualTitleRenamed("Local only".into()))
        .unwrap();
    flush_ack(&actor.handle)
        .await
        .expect("ManualTitleRenamed with remote_sync=None must not fail");
    actor.stop().await;
}

/// The title goes auto, then manual, then unpinned; the generator starts Done (as production `load` would).
/// A ContentChunk before reset must not spawn.
/// ResetTitleToAuto must `reset()` and clear the remote pin.
/// A later ContentChunk must adopt via the fallback (empty model, no live LLM).
#[tokio::test]
async fn reset_title_to_auto_then_generated_title_is_adopted() {
    use std::sync::Arc;

    use crate::auth::{AuthManager, GrokAuth};
    use crate::remote::BackendClient;
    use crate::session::export::ExportedMetadata;
    use crate::session::helpers::session_summary::title_fallback_from_user_text;
    use crate::session::persistence::PersistenceContentChunk;
    use xai_grok_test_support::MockInferenceServer;

    const AUTO: &str = "Auto first title";
    const MANUAL: &str = "Pinned manual";
    const CHUNK: &str = "fresh auto title from next chunk please";
    const SESSION_ID: &str = "rename-reset-to-auto";
    let expected_fresh = title_fallback_from_user_text(CHUNK);

    let server = MockInferenceServer::start()
        .await
        .expect("start MockInferenceServer");
    let home = tempfile::tempdir().unwrap();
    let auth = Arc::new(AuthManager::new(
        home.path(),
        crate::auth::GrokComConfig::default(),
    ));
    auth.hot_swap(GrokAuth {
        key: "writeback-test-token".into(),
        ..GrokAuth::test_default()
    });

    let info = Info {
        id: acp::SessionId::new(SESSION_ID),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    assert!(
        storage
            .set_generated_title_if_absent(&info, AUTO.to_owned())
            .await
            .unwrap()
    );
    storage
        .update_session_title(&info, MANUAL.to_owned())
        .await
        .unwrap();
    assert!(
        !storage
            .set_generated_title_if_absent(&info, "Rejected auto".into())
            .await
            .unwrap(),
        "manual pin must reject auto title before reset"
    );

    let metadata = ExportedMetadata {
        title: Some(MANUAL.into()),
        cwd: info.cwd.clone(),
        model_id: Some("test-model".into()),
        created_at: None,
        updated_at: None,
        total_messages: None,
        parent_session_id: None,
        session_kind: None,
        subagent_type: None,
        subagent_persona: None,
        subagent_role: None,
        fork_context_source: None,
        subagent_depth: None,
        title_is_manual: Some(true),
    };
    let client = BackendClient::with_base_url(server.origin()).with_auth_manager(auth);
    let remote_sync = RemoteSync::new(SESSION_ID.to_owned(), metadata, client);
    let actor = test_actor_inner(
        info.clone(),
        storage.clone(),
        Some(remote_sync),
        true, /* mark_summary_done: production load after a titled session */
    );

    let pre_reset_chunk = PersistenceContentChunk::new(vec![acp::ContentBlock::Text(
        acp::TextContent::new("should not generate while still manual and Done"),
    )]);
    actor
        .handle
        .tx
        .send(PersistenceMsg::ContentChunk(pre_reset_chunk))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();
    let summary_path = dir.path().join("summary.json");
    let still_manual: crate::session::persistence::Summary =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    assert_eq!(still_manual.display_title(), MANUAL);
    assert!(still_manual.title_is_manual);
    assert!(
        save_session_titles(&server, SESSION_ID)
            .iter()
            .filter(|t| t.title_present)
            .all(|t| t.title == MANUAL),
        "Done generator must not adopt an in-flight title before reset"
    );

    assert!(storage.reset_title_to_auto(&info).await.unwrap());
    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "after reset")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let titles = save_session_titles(&server, SESSION_ID);
        if titles
            .iter()
            .any(|t| t.title_present && t.title.is_empty() && t.title_is_manual == Some(false))
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "unpin must POST title:\"\" and title_is_manual:false (merge backends keep a prior true if omitted): {titles:?}"
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let upserted_title =
        wait_for_upserted_title(&server, SESSION_ID, std::time::Duration::from_secs(5)).await;
    assert_eq!(
        upserted_title.as_deref(),
        Some(""),
        "ClearTitle must upsert the session-row title empty, not only the metadata blob; requests={:?}",
        request_path_summary(&server)
    );

    let post_reset: crate::session::persistence::Summary =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    assert!(
        post_reset.display_title().trim().is_empty(),
        "display_title must be blank so if-absent can adopt"
    );

    let post_reset_chunk =
        PersistenceContentChunk::new(vec![acp::ContentBlock::Text(acp::TextContent::new(CHUNK))]);
    actor
        .handle
        .tx
        .send(PersistenceMsg::ContentChunk(post_reset_chunk))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "after auto")))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
    let on_disk = loop {
        let on_disk: crate::session::persistence::Summary =
            serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
        if on_disk.display_title() == expected_fresh && !on_disk.title_is_manual {
            break on_disk;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "ContentChunk after reset never adopted fallback {expected_fresh:?}; display={:?}",
                on_disk.display_title()
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(on_disk.display_title(), expected_fresh);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let titles = save_session_titles(&server, SESSION_ID);
        if titles
            .iter()
            .any(|t| t.title == expected_fresh && t.title_is_manual.is_none())
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("adopted auto title after reset never reached save_session_data: {titles:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    actor.stop().await;
}

/// A title generation from the first chunk is still running when the unpin lands.
/// Disk is already blank when the stale `GeneratedTitle` arrives, so `set_generated_title_if_absent` adopts it as auto (never re-pins).
#[tokio::test]
async fn reset_title_to_auto_adopts_in_flight_generation_as_auto() {
    use std::sync::Arc;

    use crate::session::persistence::PersistenceContentChunk;

    const MANUAL: &str = "Pinned manual";
    const CHUNK: &str = "in flight chunk text for title fallback";

    let info = Info {
        id: acp::SessionId::new("rename-reset-inflight"),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .update_session_title(&info, MANUAL.to_owned())
        .await
        .unwrap();

    let (base_url, mut in_flight) = endpoint_that_holds_the_title_request().await;
    let actor = test_actor_with_sampler(
        info.clone(),
        storage.clone(),
        None,
        false,
        title_sampler_config(&base_url),
    );

    actor
        .handle
        .tx
        .send(PersistenceMsg::ContentChunk(PersistenceContentChunk::new(
            vec![acp::ContentBlock::Text(acp::TextContent::new(CHUNK))],
        )))
        .unwrap();
    // The generation is now provably mid-request: its one open connection is held here, so no
    // `GeneratedTitle` can exist yet and the unpin below cannot lose to it.
    in_flight.wait_until_in_flight().await;
    assert!(storage.reset_title_to_auto(&info).await.unwrap());
    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let summary_path = dir.path().join("summary.json");
    let blank: crate::session::persistence::Summary =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    assert!(
        blank.display_title().trim().is_empty(),
        "the unpin did not blank disk while the generation was still open, so this is not the \
         ordering it is named for: {:?}",
        blank.display_title()
    );

    // The request dies, the generator falls back to truncated user text, and the actor adopts it.
    in_flight.release();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
    let on_disk = loop {
        let on_disk: crate::session::persistence::Summary =
            serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
        if !on_disk.display_title().trim().is_empty() {
            break on_disk;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("in-flight GeneratedTitle after unpin never adopted");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_ne!(on_disk.display_title(), MANUAL);
    assert!(
        !on_disk.title_is_manual,
        "in-flight adopt after unpin must stay auto"
    );
    actor.stop().await;
}

/// Reads the summary the actor's storage writes, as the shipped type.
fn read_summary(path: &std::path::Path) -> crate::session::persistence::Summary {
    serde_json::from_slice(&std::fs::read(path).unwrap())
        .expect("summary.json must parse as the shipped Summary")
}

/// The ordering the in-flight test above cannot produce: the generated title reaches the actor
/// while the pin is still on disk, `set_generated_title_if_absent` refuses it, and the unpin's blank
/// lands afterwards. Nothing else re-offers a refused title, so this left the session with no title
/// at all until some later turn regenerated one.
///
/// `GeneratedTitle` is sent directly because it is the message the generator's spawned task sends,
/// and the unpin's blank is a storage write that does not go through this actor at all
/// (`extensions::session_admin::reset_session_title_to_auto`). A test cannot order those two from
/// outside; the actor's own queue is the only place where the ordering exists.
#[tokio::test]
async fn unpin_replays_the_auto_title_that_lost_the_race() {
    const MANUAL: &str = "Pinned manual";
    const STALE_AUTO: &str = "Title that lost the race";

    let info = Info {
        id: acp::SessionId::new("unpin-replays-held-title"),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .update_session_title(&info, MANUAL.to_owned())
        .await
        .unwrap();

    let actor = test_actor(info.clone(), storage.clone());
    let summary_path = dir.path().join("summary.json");

    actor
        .handle
        .tx
        .send(PersistenceMsg::GeneratedTitle(STALE_AUTO.into()))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();
    let after_reject = read_summary(&summary_path);
    assert_eq!(
        after_reject.display_title(),
        MANUAL,
        "a refused auto title must not reach disk while the pin stands"
    );
    assert!(
        after_reject.title_is_manual,
        "the refusal must not demote the manual pin"
    );

    assert!(storage.reset_title_to_auto(&info).await.unwrap());
    let blank = read_summary(&summary_path);
    assert!(
        blank.display_title().trim().is_empty(),
        "precondition for the lost update: the unpin must blank the title, got {:?}",
        blank.display_title()
    );

    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let replayed = read_summary(&summary_path);
    assert_eq!(
        replayed.display_title(),
        STALE_AUTO,
        "the unpin left the session titleless; the refused auto title was never replayed"
    );
    assert!(
        !replayed.title_is_manual,
        "a replayed auto title is still an auto title, not a re-pin"
    );
    actor.stop().await;
}

/// The replay is bounded to the unpin. A refused auto title must not sneak in on some later
/// message, or the manual `/rename` that beat it would silently lose.
#[tokio::test]
async fn rejected_auto_title_is_not_adopted_without_an_unpin() {
    const MANUAL: &str = "Pinned manual";
    const STALE_AUTO: &str = "Title that lost the race";

    let info = Info {
        id: acp::SessionId::new("held-title-waits-for-unpin"),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .update_session_title(&info, MANUAL.to_owned())
        .await
        .unwrap();

    let actor = test_actor(info.clone(), storage.clone());
    let summary_path = dir.path().join("summary.json");

    actor
        .handle
        .tx
        .send(PersistenceMsg::GeneratedTitle(STALE_AUTO.into()))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    for text in ["still pinned", "still pinned again"] {
        actor
            .handle
            .tx
            .send(PersistenceMsg::Update(neutral_update(&info, text)))
            .unwrap();
        flush_ack(&actor.handle).await.unwrap();
        let on_disk = read_summary(&summary_path);
        assert_eq!(
            on_disk.display_title(),
            MANUAL,
            "the held title must not be adopted by an unrelated message ({text:?})"
        );
        assert!(on_disk.title_is_manual, "{text:?} must not demote the pin");
    }
    actor.stop().await;
}

/// The replayed title is provisional, not a freeze. The same unpin reopens the whole-conversation
/// refresh (`SessionCommand::TitleRenamed { manual: false }`), which lands as `RegenerateTitle` and
/// is not gated on an empty title, so an early title cannot pin itself in place.
#[tokio::test]
async fn replayed_held_title_is_overwritten_by_the_refresh_it_reopens() {
    const MANUAL: &str = "Pinned manual";
    const STALE_AUTO: &str = "Title that lost the race";
    const REFRESHED: &str = "Title from the whole conversation";

    let info = Info {
        id: acp::SessionId::new("held-title-then-refresh"),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .update_session_title(&info, MANUAL.to_owned())
        .await
        .unwrap();

    let actor = test_actor(info.clone(), storage.clone());
    let summary_path = dir.path().join("summary.json");

    actor
        .handle
        .tx
        .send(PersistenceMsg::GeneratedTitle(STALE_AUTO.into()))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();
    assert!(storage.reset_title_to_auto(&info).await.unwrap());
    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();
    assert_eq!(read_summary(&summary_path).display_title(), STALE_AUTO);

    actor
        .handle
        .tx
        .send(PersistenceMsg::RegenerateTitle(REFRESHED.into()))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let on_disk = read_summary(&summary_path);
    assert_eq!(
        on_disk.display_title(),
        REFRESHED,
        "the replayed title must stay replaceable by the refresh the unpin reopens"
    );
    assert!(!on_disk.title_is_manual);
    actor.stop().await;
}

/// The hold is spent by the replay. A later unpin that nothing raced must not resurrect a title from
/// an earlier generation; the whole-conversation refresh that the unpin reopens is what titles the
/// session in that case.
#[tokio::test]
async fn a_spent_held_title_is_not_replayed_by_a_later_unpin() {
    const MANUAL_ONE: &str = "Pinned first";
    const MANUAL_TWO: &str = "Pinned again";
    const STALE_AUTO: &str = "Title that lost the first race";

    let info = Info {
        id: acp::SessionId::new("unpin-replays-held-title-once"),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let summary_path = dir.path().join("summary.json");

    let actor = test_actor(info.clone(), storage.clone());

    storage
        .update_session_title(&info, MANUAL_ONE.to_owned())
        .await
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::GeneratedTitle(STALE_AUTO.into()))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();
    assert!(storage.reset_title_to_auto(&info).await.unwrap());
    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();
    assert_eq!(read_summary(&summary_path).display_title(), STALE_AUTO);

    // Re-pin, then unpin with nothing in flight this time.
    storage
        .update_session_title(&info, MANUAL_TWO.to_owned())
        .await
        .unwrap();
    assert!(storage.reset_title_to_auto(&info).await.unwrap());
    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let on_disk = read_summary(&summary_path);
    assert!(
        on_disk.display_title().trim().is_empty(),
        "a consumed hold must not resurface as the title of a later unpin, got {:?}",
        on_disk.display_title()
    );
    assert!(!on_disk.title_is_manual);
    actor.stop().await;
}

/// A loopback endpoint that accepts the title request, reads just enough of it to be sure which
/// request it is, and then withholds any response until the guard releases it.
///
/// The withheld request is what makes "the title generation is still in flight" a fact the test
/// enforces rather than an ordering it hopes for. Left to itself, the generator's request fails
/// inside the same microsecond it is spawned, so the `GeneratedTitle` can reach the actor before
/// or after the unpin depending on how the runtime happens to interleave the two tasks.
///
/// Connections that are not the title `POST` are closed and skipped on purpose: the sampler dials
/// an origin once before its first real request to prewarm the shared transport pool, and that
/// `GET` must never be taken for the request under test.
async fn endpoint_that_holds_the_title_request() -> (String, HeldTitleRequest) {
    use tokio::io::AsyncReadExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = tokio::sync::mpsc::channel::<()>(1);
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            // The method is the first token of the request line, so five bytes settle it.
            let mut head = [0u8; 5];
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                socket.read_exact(&mut head),
            )
            .await;
            if read.is_err() || read.unwrap().is_err() || &head != b"POST " {
                continue;
            }
            if seen_tx.send(()).await.is_err() {
                return;
            }
            // No response is ever written. Dropping the socket when the guard fires is what
            // ends the request, which is the failure the fallback title comes from.
            let _ = release_rx.await;
            return;
        }
    });
    (
        base_url,
        HeldTitleRequest {
            seen: seen_rx,
            release: Some(release_tx),
        },
    )
}

struct HeldTitleRequest {
    seen: tokio::sync::mpsc::Receiver<()>,
    release: Option<tokio::sync::oneshot::Sender<()>>,
}

impl HeldTitleRequest {
    /// Waits until the title request has arrived and been read.
    /// Fails the test rather than degrading quietly: a generator that never dials the endpoint is
    /// not on the in-flight path this fixture exists to create, and a test that passed anyway
    /// would be worse than one that failed here.
    async fn wait_until_in_flight(&mut self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.seen.recv())
            .await
            .expect("the title request never reached the loopback endpoint")
            .expect("the endpoint task ended before the title request arrived");
    }

    /// Closes the held connection, so the request fails and the generator falls back.
    fn release(mut self) {
        if let Some(tx) = self.release.take() {
            let _ = tx.send(());
        }
    }
}

/// A config that makes the title generator dial `base_url` for real.
/// An empty `base_url` is not the only thing that keeps the default config in-process: the empty
/// `model` and missing credentials are what turn the request into a builder error before any
/// socket is opened, which is exactly why the default path cannot express "still running".
/// `max_retries: 0` keeps the aborted request from being re-dialled onto a fresh connection.
fn title_sampler_config(base_url: &str) -> xai_grok_sampler::SamplerConfig {
    xai_grok_sampler::SamplerConfig {
        api_key: Some("test-key".to_owned()),
        base_url: base_url.to_owned(),
        model: "test-model".to_owned(),
        api_backend: xai_grok_sampler::ApiBackend::ChatCompletions,
        auth_scheme: xai_grok_sampler::AuthScheme::Bearer,
        max_retries: Some(0),
        context_window: 8192,
        ..Default::default()
    }
}

/// An unpin while the session is not resident only patches disk.
/// The next load sees a blank `display_title()` so the generator stays Idle and a ContentChunk adopts.
#[tokio::test]
async fn non_resident_reset_then_load_regenerates() {
    use std::sync::Arc;

    use crate::session::helpers::session_summary::title_fallback_from_user_text;
    use crate::session::persistence::PersistenceContentChunk;

    const CHUNK: &str = "dormant session next turn title text";
    let expected = title_fallback_from_user_text(CHUNK);

    let info = Info {
        id: acp::SessionId::new("rename-reset-dormant-load"),
        cwd: "/test".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    assert!(
        storage
            .set_generated_title_if_absent(&info, "Auto Title".into())
            .await
            .unwrap()
    );
    storage
        .update_session_title(&info, "Manual Title".into())
        .await
        .unwrap();
    assert!(storage.reset_title_to_auto(&info).await.unwrap());

    let summary_path = dir.path().join("summary.json");
    let after_reset: crate::session::persistence::Summary =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    let has_title = !after_reset.display_title().is_empty();
    assert!(
        !has_title,
        "production load would mark_done() if display_title stayed set"
    );

    let actor = test_actor_inner(info.clone(), storage, None, has_title);
    actor
        .handle
        .tx
        .send(PersistenceMsg::ContentChunk(PersistenceContentChunk::new(
            vec![acp::ContentBlock::Text(acp::TextContent::new(CHUNK))],
        )))
        .unwrap();
    flush_ack(&actor.handle).await.unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
    let on_disk = loop {
        let on_disk: crate::session::persistence::Summary =
            serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
        if on_disk.display_title() == expected && !on_disk.title_is_manual {
            break on_disk;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "load after non-resident unpin never adopted {expected:?}; display={:?}",
                on_disk.display_title()
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(on_disk.display_title(), expected);
    actor.stop().await;
}

#[tokio::test]
async fn reset_title_to_auto_is_noop_without_remote_sync() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("reset-local-only"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_explicit_session_dir(
        dir.path().to_path_buf(),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .update_session_title(&info, "Manual".into())
        .await
        .unwrap();
    storage.reset_title_to_auto(&info).await.unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(PersistenceMsg::ResetTitleToAuto)
        .unwrap();
    flush_ack(&actor.handle)
        .await
        .expect("ResetTitleToAuto with remote_sync=None must not fail");
    actor.stop().await;
}

#[tokio::test]
async fn successful_append_clears_disk_full_latch() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("disk-full-clear"),
        cwd: "/test".into(),
    };
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let fail_flag = fail.clone();
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().to_path_buf(),
        move |_| {
            if fail_flag.load(std::sync::atomic::Ordering::SeqCst) {
                Err(io::Error::from(io::ErrorKind::StorageFull))
            } else {
                Ok(())
            }
        },
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "chunk")))
        .unwrap();
    assert!(flush_ack(&actor.handle).await.is_err());
    assert!(actor.handle.is_disk_full());

    fail.store(false, std::sync::atomic::Ordering::SeqCst);
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "recovered")))
        .unwrap();
    assert!(flush_ack(&actor.handle).await.is_ok());
    assert!(!actor.handle.is_disk_full());
    actor.stop().await;
}

#[tokio::test]
async fn successful_probe_writable_clears_disk_full_latch() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("disk-full-probe"),
        cwd: "/test".into(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().to_path_buf(),
        |_| Err(io::Error::from(io::ErrorKind::StorageFull)),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "chunk")))
        .unwrap();
    assert!(flush_ack(&actor.handle).await.is_err());
    assert!(actor.handle.is_disk_full());

    assert!(probe_writable(&actor.handle).await.is_ok());
    assert!(!actor.handle.is_disk_full());
    actor.stop().await;
}

#[cfg(unix)]
mod prompt_file_tests {
    use super::*;
    use crate::test_support::unix_mode;

    #[test]
    fn prompt_file_dir_chain_is_owner_only() {
        let home = tempfile::TempDir::new().unwrap();
        let info = Info {
            id: agent_client_protocol::SessionId::new("prompt-perm-test"),
            cwd: "/some/project".to_string(),
        };

        let path = get_prompt_file_path_in(home.path(), &info, 0);

        // The chain below prompts/ is ensure_owner_only_session_dir_in's job, pinned by ensure_owner_only_session_dir_tightens_chain
        // Only the prompts/ level is this path's own creation
        let prompts_dir = path.parent().unwrap();
        assert_eq!(unix_mode(prompts_dir), 0o700, "prompts dir must be 0700");
    }

    /// ensure_owner_only_session_dir_in is the dir creator for chat-kind (noop-persistence) writers.
    #[test]
    fn ensure_owner_only_session_dir_tightens_chain() {
        let home = tempfile::TempDir::new().unwrap();
        let info = Info {
            id: agent_client_protocol::SessionId::new("chat-kind-perm-test"),
            cwd: "/some/project".to_string(),
        };

        let dir = ensure_owner_only_session_dir_in(home.path(), &info).unwrap();

        assert_eq!(unix_mode(&dir), 0o700, "session dir must be 0700");
        assert_eq!(
            unix_mode(dir.parent().unwrap()),
            0o700,
            "<encoded-cwd> dir must be 0700"
        );
        assert_eq!(
            unix_mode(&home.path().join("sessions")),
            0o700,
            "sessions root must be 0700"
        );
    }

    #[test]
    fn ensure_owner_only_session_dir_syncs_each_parent_that_gained_an_entry() {
        let home = tempfile::TempDir::new().unwrap();
        let info = Info {
            id: agent_client_protocol::SessionId::new("chat-kind-sync-test"),
            cwd: "/some/project".to_string(),
        };
        let dir = session_dir_in(home.path(), &info);
        let synced = std::cell::RefCell::new(Vec::new());

        ensure_owner_only_session_dir_in_with(
            home.path(),
            &info,
            |path| {
                synced.borrow_mut().push(path.to_path_buf());
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();

        assert!(dir.is_dir());
        let synced = synced.borrow();
        assert!(
            synced.iter().any(|path| path == dir.parent().unwrap()),
            "encoded-cwd must be synced after gaining the session direntry, got {synced:?}"
        );
        assert!(
            synced
                .iter()
                .any(|path| path == &home.path().join("sessions")),
            "sessions root must be synced after gaining encoded-cwd, got {synced:?}"
        );
    }

    #[test]
    fn ensure_owner_only_session_dir_does_not_return_ok_when_a_parent_sync_fails() {
        let home = tempfile::TempDir::new().unwrap();
        let info = Info {
            id: agent_client_protocol::SessionId::new("chat-kind-sync-fail"),
            cwd: "/some/project".to_string(),
        };

        let error = ensure_owner_only_session_dir_in_with(
            home.path(),
            &info,
            |_| Err(std::io::Error::other("directory barrier failed")),
            |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "directory barrier failed");
        assert!(
            session_dir_in(home.path(), &info).is_dir(),
            "create must still leave the session dir so a retry can finish the barrier"
        );
    }

    #[test]
    fn ensure_owner_only_session_dir_skips_parent_sync_on_a_populated_session() {
        let home = tempfile::TempDir::new().unwrap();
        let info = Info {
            id: agent_client_protocol::SessionId::new("chat-kind-occupied"),
            cwd: "/some/project".to_string(),
        };
        let dir = ensure_owner_only_session_dir_in(home.path(), &info).unwrap();
        std::fs::write(dir.join("summary.json"), b"{}").unwrap();

        let synced = std::cell::RefCell::new(Vec::new());
        ensure_owner_only_session_dir_in_with(
            home.path(),
            &info,
            |path| {
                synced.borrow_mut().push(path.to_path_buf());
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();
        assert!(
            synced.borrow().is_empty(),
            "occupied resume must not fsync ancestors, got {:?}",
            synced.borrow()
        );
    }

    /// Hash-encoded `.cwd` contents must hit stable media before the parent dir sync that makes the direntry durable.
    /// Otherwise power loss can freeze a present-but-torn marker and path recovery cannot fall back to missing.
    #[test]
    fn hash_encoded_cwd_marker_is_synced_before_parent_dir_sync() {
        let home = tempfile::TempDir::new().unwrap();
        // URL-encoded form exceeds 255 bytes, so encode writes `.cwd`.
        let long_cwd = format!("/Users/test/{}", "中".repeat(80));
        let info = Info {
            id: agent_client_protocol::SessionId::new("cwd-marker-sync"),
            cwd: long_cwd,
        };
        let cwd_dir = crate::util::grok_home::sessions_cwd_dir_in(home.path(), &info.cwd);
        let cwd_file = cwd_dir.join(".cwd");

        let events = std::cell::RefCell::new(Vec::new());
        ensure_owner_only_session_dir_in_with(
            home.path(),
            &info,
            |path| {
                events.borrow_mut().push(format!("dir:{}", path.display()));
                Ok(())
            },
            |_file| {
                events
                    .borrow_mut()
                    .push(format!("file:{}", cwd_file.display()));
                Ok(())
            },
        )
        .unwrap();

        assert!(
            cwd_file.is_file(),
            "hash-encoded cwd must write a .cwd marker"
        );
        let events = events.borrow();
        let file_pos = events
            .iter()
            .position(|event| event == &format!("file:{}", cwd_file.display()))
            .unwrap_or_else(|| {
                panic!("cwd marker file must be fsynced before parent dir sync, got {events:?}")
            });
        let dir_pos = events
            .iter()
            .position(|event| event == &format!("dir:{}", cwd_dir.display()))
            .unwrap_or_else(|| {
                panic!("encoded-cwd parent must be synced after gaining .cwd, got {events:?}")
            });
        assert!(
            file_pos < dir_pos,
            ".cwd file sync must happen before the parent-dir sync that would freeze the direntry, got {events:?}"
        );
    }
}
