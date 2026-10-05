//! A prompt turn is a live run, not a request/response pair.
//!
//! Every assertion here drives the shipped [`Engine`] through the same two entry
//! points the Web host uses: [`Engine::subscribe`] for the per-connection event
//! stream and [`Engine::dispatch_to`] for the client message. What they pin down is
//! the part that a `Vec<ServerMessage>` return value cannot express -- that chunks
//! leave while the Producer is still producing them, that `cancel` reaches a run
//! that has not finished, and that a connection which stops reading is cut off
//! rather than quietly truncated.

use chaos_engine::{ClientMessage, Engine, EventStream, PromptAdapter, ServerMessage};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};
use uuid::Uuid;

/// Long enough for a contended lock on a loaded CI runner, short enough that a
/// hang is reported as the missing event it is rather than as a job timeout.
const WAIT: Duration = Duration::from_secs(20);

async fn next_event(stream: &mut EventStream, what: &str) -> ServerMessage {
    match tokio::time::timeout(WAIT, stream.recv()).await {
        Ok(Some(event)) => event,
        Ok(None) => panic!("{what}: the engine stopped delivering events to this connection"),
        Err(_) => panic!("{what}: no event within {WAIT:?}"),
    }
}

/// Nothing is owed to this connection.
///
/// Not a race: delivery happens synchronously inside `dispatch_to`, so by the time
/// the caller has its answer the other connection's buffer is already final and a
/// missed event would have to arrive on a channel nothing is writing to.
async fn expect_quiet(stream: &mut EventStream, what: &str) {
    if let Ok(event) = tokio::time::timeout(Duration::from_millis(250), stream.recv()).await {
        panic!("{what}: an event arrived that this connection never asked for: {event:?}");
    }
}

/// The same, for a connection that is legitimately owed host-wide frames (the
/// workspace list reaches every socket): only an event naming this session is proof
/// it was told about somebody else's conversation.
async fn expect_quiet_about(stream: &mut EventStream, session_id: Uuid, what: &str) {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        if let Ok(Some(event)) = tokio::time::timeout(remaining, stream.recv()).await {
            assert_ne!(event.session_id(), Some(session_id), "{what}: {event:?}");
        }
    }
}

fn create_session(engine: &Engine) -> Uuid {
    let events = engine.handle(ClientMessage::CreateSession {
        client_msg_id: new_id("create"),
        workspace_id: None,
    });
    match events.as_slice() {
        [ServerMessage::SessionCreated { session_id, .. }] => *session_id,
        other => panic!("unexpected {other:?}"),
    }
}

fn new_id(kind: &str) -> String {
    format!("{kind}-{}", Uuid::new_v4())
}

fn submit(session_id: Uuid, prompt: &str) -> ClientMessage {
    ClientMessage::Submit {
        client_msg_id: new_id("submit"),
        session_id,
        prompt: prompt.into(),
    }
}

/// Runs one client message the way the Web host does: on a thread with no runtime,
/// with the answers addressed to a subscription and the return path ignored.
fn dispatch(engine: Engine, message: ClientMessage, sink: &chaos_engine::EventSink) {
    engine.dispatch_to(message, Some(sink), &mut |_event| {});
}

/// The transcript the engine would hand a reconnecting client.
async fn transcript(engine: &Engine, session_id: Uuid) -> Vec<(String, String)> {
    let engine = engine.clone();
    let events = tokio::time::timeout(
        WAIT,
        tokio::task::spawn_blocking(move || {
            engine.handle(ClientMessage::Snapshot {
                client_msg_id: new_id("snapshot"),
                session_id,
                workspace_id: None,
            })
        }),
    )
    .await
    .expect("the snapshot blocked behind a prompt run")
    .expect("the snapshot thread panicked");
    match events.as_slice() {
        [ServerMessage::SessionSnapshot { messages, .. }] => messages
            .iter()
            .map(|m| (m.role.clone(), m.text.clone()))
            .collect(),
        other => panic!("unexpected {other:?}"),
    }
}

// --------------------------------------------------------------------------------
// Test adapters
// --------------------------------------------------------------------------------

/// Streams its second half only once a subscriber has proved it saw the first.
///
/// The proof can only come from outside the adapter call, so if the engine ever
/// buffers the reply and hands it over at the end, this adapter times out and the
/// test fails with the engine's own words rather than with a passing assertion.
struct WaitsForTheFirstChunk {
    released: Mutex<Receiver<()>>,
    produced: Arc<AtomicUsize>,
    delivered: Arc<AtomicBool>,
}

impl PromptAdapter for WaitsForTheFirstChunk {
    fn run_prompt(&self, _prompt: &str) -> Result<Vec<String>, String> {
        Err("this adapter only has the streaming form".into())
    }

    fn run_prompt_stream(
        &self,
        _prompt: &str,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        self.produced.fetch_add(1, Ordering::SeqCst);
        if !emit("前半") {
            return Ok(());
        }
        match self
            .released
            .lock()
            .expect("adapter lock")
            .recv_timeout(WAIT)
        {
            Ok(()) => self.delivered.store(true, Ordering::SeqCst),
            Err(_) => return Err("第一个分块在超时前没有到达订阅者".into()),
        }
        self.produced.fetch_add(1, Ordering::SeqCst);
        if !emit("后半") {
            return Ok(());
        }
        Ok(())
    }
}

/// Keeps producing until the engine tells it to stop.
///
/// `SELF_LIMIT` chunks at 5 ms each is ~20 s, so a run this test ends in well under
/// a second can only have ended because somebody interrupted it.
struct EndlessProducer {
    produced: Arc<AtomicUsize>,
    stop_reason: Arc<Mutex<String>>,
}

const SELF_LIMIT: usize = 4_000;

impl PromptAdapter for EndlessProducer {
    fn run_prompt(&self, _prompt: &str) -> Result<Vec<String>, String> {
        Err("this adapter only has the streaming form".into())
    }

    fn run_prompt_stream(
        &self,
        _prompt: &str,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        loop {
            let produced = self.produced.fetch_add(1, Ordering::SeqCst) + 1;
            if !emit(&format!("第{produced}块")) {
                *self.stop_reason.lock().expect("adapter lock") = "stopped".into();
                return Ok(());
            }
            if produced >= SELF_LIMIT {
                *self.stop_reason.lock().expect("adapter lock") = "limit".into();
                return Ok(());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Streams one chunk, then holds the turn open until the test lets it finish.
struct HoldsTheTurnOpen {
    released: Mutex<Receiver<()>>,
}

impl PromptAdapter for HoldsTheTurnOpen {
    fn run_prompt(&self, _prompt: &str) -> Result<Vec<String>, String> {
        Err("this adapter only has the streaming form".into())
    }

    fn run_prompt_stream(
        &self,
        _prompt: &str,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        if !emit("还在写") {
            return Ok(());
        }
        let _ = self
            .released
            .lock()
            .expect("adapter lock")
            .recv_timeout(WAIT);
        if !emit("写完了") {
            return Ok(());
        }
        Ok(())
    }
}

/// Streams a chunk, then falls over on the first answer and behaves on the second.
///
/// A Provider adapter is third-party code, and a panic inside one is a bug in the
/// Provider, not a reason for this process to keep a session busy forever.
struct PanicsOnTheFirstAnswer {
    panicked: AtomicBool,
}

impl PromptAdapter for PanicsOnTheFirstAnswer {
    fn run_prompt(&self, _prompt: &str) -> Result<Vec<String>, String> {
        Err("this adapter only has the streaming form".into())
    }

    fn run_prompt_stream(
        &self,
        _prompt: &str,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        emit("写了一半");
        if !self.panicked.swap(true, Ordering::SeqCst) {
            panic!("the adapter fell over mid-answer");
        }
        emit("这一次写完了");
        Ok(())
    }
}

/// Holds every turn it is given until the test releases them all.
struct HoldsEveryTurnItIsGiven {
    release: Arc<AtomicBool>,
}

impl PromptAdapter for HoldsEveryTurnItIsGiven {
    fn run_prompt(&self, _prompt: &str) -> Result<Vec<String>, String> {
        Err("this adapter only has the streaming form".into())
    }

    fn run_prompt_stream(
        &self,
        _prompt: &str,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        let started = Instant::now();
        while !self.release.load(Ordering::SeqCst) && started.elapsed() < WAIT {
            thread::sleep(Duration::from_millis(5));
        }
        emit("排队结束了");
        Ok(())
    }
}

// --------------------------------------------------------------------------------
// Streaming
// --------------------------------------------------------------------------------

#[tokio::test]
async fn chunks_reach_the_subscriber_while_the_adapter_is_still_producing() {
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let produced = Arc::new(AtomicUsize::new(0));
    let delivered = Arc::new(AtomicBool::new(false));
    let engine = Engine::with_adapter(WaitsForTheFirstChunk {
        released: Mutex::new(release_rx),
        produced: Arc::clone(&produced),
        delivered: Arc::clone(&delivered),
    });
    let session_id = create_session(&engine);
    let mut stream = engine.subscribe();
    let sink = stream.sink();
    let running = {
        let engine = engine.clone();
        let message = submit(session_id, "说点什么");
        let sink = sink.clone();
        thread::spawn(move || dispatch(engine, message, &sink))
    };

    let ack = next_event(&mut stream, "第一个事件").await;
    assert!(matches!(ack, ServerMessage::Ack { .. }), "{ack:?}");
    let first = next_event(&mut stream, "第一个分块").await;
    assert!(
        matches!(&first, ServerMessage::TextDelta { text, .. } if text == "前半"),
        "{first:?}"
    );
    assert_eq!(
        produced.load(Ordering::SeqCst),
        1,
        "the subscriber saw a chunk the adapter had not produced yet"
    );

    // The state lock used to be held across the adapter call, which queued every
    // other client message behind the reply. One that is served while the adapter
    // is parked proves the turn no longer blocks the engine.
    let snapshot_engine = engine.clone();
    let snapshot = tokio::time::timeout(
        WAIT,
        tokio::task::spawn_blocking(move || {
            snapshot_engine.handle(ClientMessage::Snapshot {
                client_msg_id: new_id("snapshot"),
                session_id,
                workspace_id: None,
            })
        }),
    )
    .await
    .expect("a client message queued behind the running prompt")
    .expect("the snapshot thread panicked");
    assert!(
        matches!(snapshot.as_slice(), [ServerMessage::SessionSnapshot { .. }]),
        "{snapshot:?}"
    );

    release_tx.send(()).expect("the adapter is still waiting");
    let second = next_event(&mut stream, "第二个分块").await;
    assert!(
        matches!(&second, ServerMessage::TextDelta { text, .. } if text == "后半"),
        "{second:?}"
    );
    let terminal = next_event(&mut stream, "终态事件").await;
    let completed_at = match terminal {
        ServerMessage::Completed { sequence, .. } => sequence,
        other => panic!("expected Completed, got {other:?}"),
    };
    let (first_at, second_at) = match (&first, &second) {
        (
            ServerMessage::TextDelta { sequence: one, .. },
            ServerMessage::TextDelta { sequence: two, .. },
        ) => (*one, *two),
        _ => unreachable!("both asserted to be deltas above"),
    };
    assert_eq!(
        second_at,
        first_at + 1,
        "the two chunks were not consecutive"
    );
    assert!(
        completed_at > second_at,
        "Completed was numbered before the chunks"
    );
    assert!(
        delivered.load(Ordering::SeqCst),
        "the adapter never learned the chunk arrived"
    );
    assert_eq!(produced.load(Ordering::SeqCst), 2);

    running.join().expect("the prompt thread panicked");
    let messages = transcript(&engine, session_id).await;
    assert_eq!(
        messages,
        vec![
            ("user".to_string(), "说点什么".to_string()),
            ("assistant".to_string(), "前半后半".to_string()),
        ],
        "the stored transcript is not the one the connection watched arrive"
    );
}

// --------------------------------------------------------------------------------
// Cancellation
// --------------------------------------------------------------------------------

#[tokio::test]
async fn cancel_stops_the_producer_instead_of_only_booking_keeping() {
    let produced = Arc::new(AtomicUsize::new(0));
    let stop_reason = Arc::new(Mutex::new(String::new()));
    let finished = Arc::new(AtomicBool::new(false));
    let engine = Engine::with_adapter(EndlessProducer {
        produced: Arc::clone(&produced),
        stop_reason: Arc::clone(&stop_reason),
    });
    let session_id = create_session(&engine);
    let mut stream = engine.subscribe();
    let sink = stream.sink();
    let done = Arc::clone(&finished);
    let running = {
        let engine = engine.clone();
        let message = submit(session_id, "一直说下去");
        let sink = sink.clone();
        thread::spawn(move || {
            dispatch(engine, message, &sink);
            done.store(true, Ordering::SeqCst);
        })
    };

    let ack = next_event(&mut stream, "Ack").await;
    assert!(matches!(ack, ServerMessage::Ack { .. }), "{ack:?}");
    let first = next_event(&mut stream, "第一个分块").await;
    assert!(
        matches!(first, ServerMessage::TextDelta { .. }),
        "{first:?}"
    );
    // The chunk waited for above is part of what the connection was shown, so it
    // counts toward the transcript comparison at the end of the test.
    let mut shown = String::new();
    if let ServerMessage::TextDelta { ref text, .. } = first {
        shown.push_str(text);
    }

    // The request that started the turn is over, and its producer is still going.
    // That is what the run being moved off the request is for: a connection still
    // inside the request it was given cannot carry the `cancel` below, the file
    // panel, or the next tab's prompt.
    for _ in 0..400 {
        if finished.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        finished.load(Ordering::SeqCst),
        "the request was still being handled after its first chunk reached the socket, \
         so the connection was busy producing its own answer"
    );
    assert!(
        produced.load(Ordering::SeqCst) < SELF_LIMIT,
        "the producer had already run to its own limit, so there was no live run to cancel"
    );

    let cancel_engine = engine.clone();
    let cancelled = tokio::time::timeout(
        WAIT,
        tokio::task::spawn_blocking(move || {
            cancel_engine.handle(ClientMessage::Cancel {
                client_msg_id: new_id("cancel"),
                session_id,
            })
        }),
    )
    .await
    .expect("cancel queued behind the prompt it was asked to stop")
    .expect("the cancel thread panicked");
    assert!(
        matches!(cancelled.as_slice(),
            [ServerMessage::Ack { .. }, ServerMessage::Audit { action, outcome, .. }]
                if action == "cancel" && outcome == "interrupted"),
        "a cancel that reaches a live run must not also claim the turn is over: {cancelled:?}"
    );
    assert!(
        produced.load(Ordering::SeqCst) < SELF_LIMIT,
        "the run ended on its own before the cancel, so this test proved nothing about cancel"
    );

    let terminal = loop {
        let event = next_event(&mut stream, "取消后的终态事件").await;
        match event {
            ServerMessage::TextDelta { ref text, .. } => shown.push_str(text),
            ServerMessage::Completed { .. } | ServerMessage::Cancelled { .. } => break event,
            // The cancel's own audit entry, and anything else the engine chose to
            // say while the run wound down. The turn is not over until the branch
            // above fires, which is the thing under test.
            _ => continue,
        }
    };
    assert!(
        matches!(terminal, ServerMessage::Cancelled { .. }),
        "an interrupted turn must not report Completed: {terminal:?}"
    );
    assert_eq!(
        stop_reason.lock().expect("adapter lock").as_str(),
        "stopped",
        "the producer ran to its own limit; nothing interrupted it"
    );
    let produced_total = produced.load(Ordering::SeqCst);
    assert!(
        produced_total < SELF_LIMIT / 4,
        "the producer emitted {produced_total} chunks after the cancel was accepted"
    );

    running.join().expect("the prompt thread panicked");
    assert!(
        finished.load(Ordering::SeqCst),
        "the run never reported itself over"
    );
    let messages = transcript(&engine, session_id).await;
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(
        messages[1].1, shown,
        "the stored transcript is not the text the connection was shown"
    );
    assert!(!shown.is_empty(), "the cancel arrived before any chunk did");
}

#[tokio::test]
async fn a_cancel_with_nothing_running_is_its_own_answer() {
    let engine = Engine::new();
    let session_id = create_session(&engine);
    let events = engine.handle(ClientMessage::Cancel {
        client_msg_id: new_id("cancel"),
        session_id,
    });
    assert!(
        matches!(events.as_slice(),
            [ServerMessage::Ack { .. }, ServerMessage::Cancelled { .. }, ServerMessage::Audit { outcome, .. }]
                if outcome == "accepted"),
        "{events:?}"
    );
}

// --------------------------------------------------------------------------------
// One session, one run at a time
// --------------------------------------------------------------------------------

#[tokio::test]
async fn a_second_prompt_is_refused_while_the_first_is_running_and_after_it_is_not() {
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let engine = Engine::with_adapter(HoldsTheTurnOpen {
        released: Mutex::new(release_rx),
    });
    let session_id = create_session(&engine);
    let mut stream = engine.subscribe();
    let sink = stream.sink();
    let running = {
        let engine = engine.clone();
        let message = submit(session_id, "第一轮");
        let sink = sink.clone();
        thread::spawn(move || dispatch(engine, message, &sink))
    };

    let ack = next_event(&mut stream, "Ack").await;
    assert!(matches!(ack, ServerMessage::Ack { .. }), "{ack:?}");
    let first = next_event(&mut stream, "第一个分块").await;
    assert!(
        matches!(first, ServerMessage::TextDelta { .. }),
        "{first:?}"
    );

    let busy_engine = engine.clone();
    let busy = tokio::time::timeout(
        WAIT,
        tokio::task::spawn_blocking(move || busy_engine.handle(submit(session_id, "插队"))),
    )
    .await
    .expect("the second submit blocked instead of being refused")
    .expect("the second submit thread panicked");
    assert_eq!(
        busy.len(),
        1,
        "a refused turn must not also record a prompt: {busy:?}"
    );
    assert!(
        matches!(&busy[0], ServerMessage::Error { code, .. } if code == "session_busy"),
        "{busy:?}"
    );

    release_tx.send(()).expect("the adapter is still waiting");
    running.join().expect("the prompt thread panicked");
    loop {
        match next_event(&mut stream, "第一轮收尾的事件").await {
            ServerMessage::Completed { .. } => break,
            ServerMessage::TextDelta { .. } => continue,
            other => panic!("expected the first turn to finish, got {other:?}"),
        }
    }

    let after = engine.handle(submit(session_id, "第二轮"));
    assert!(
        matches!(after.first(), Some(ServerMessage::Ack { .. })),
        "the session stayed locked after its run ended: {after:?}"
    );
    let messages = transcript(&engine, session_id).await;
    assert!(
        messages
            .iter()
            .any(|(role, text)| role == "user" && text == "第一轮")
            && !messages.iter().any(|(_, text)| text == "插队"),
        "the refused prompt reached the transcript: {messages:?}"
    );
}

// --------------------------------------------------------------------------------
// Who is allowed to see what
// --------------------------------------------------------------------------------

#[tokio::test]
async fn an_answer_reaches_only_the_connection_that_asked_for_it() {
    let directory = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        directory.path().join("a.txt"),
        "only one tab asked for this",
    )
    .unwrap();
    let engine = Engine::with_workspace(directory.path()).expect("workspace");
    let mut first = engine.subscribe();
    let first_sink = first.sink();
    let mut second = engine.subscribe();
    let second_sink = second.sink();

    let created = {
        let engine = engine.clone();
        let sink = first_sink.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::CreateSession {
                    client_msg_id: new_id("create"),
                    workspace_id: None,
                },
                &sink,
            )
        })
    };
    let session_id = match next_event(&mut first, "SessionCreated").await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("expected SessionCreated, got {other:?}"),
    };
    expect_quiet(&mut second, "the other connection did not create a session").await;
    created.await.expect("the create thread panicked");

    let listing = {
        let engine = engine.clone();
        let sink = first_sink.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::ListFiles {
                    client_msg_id: new_id("list"),
                    relative_path: ".".into(),
                },
                &sink,
            )
        })
    };
    let listed = next_event(&mut first, "FilesListed").await;
    assert!(
        matches!(listed, ServerMessage::FilesListed { .. }),
        "{listed:?}"
    );
    expect_quiet(&mut second, "one tab's file listing is not another tab's").await;
    listing.await.expect("the listing thread panicked");

    // What happened in the session, by contrast, is something every connection
    // following that session has to see. Following is not automatic: the second
    // connection has to name the session, which is what a second tab does when it
    // opens the same conversation. Until it does, this session's events are not
    // its data to show.
    let following = {
        let engine = engine.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::Snapshot {
                    client_msg_id: new_id("snapshot"),
                    session_id,
                    workspace_id: None,
                },
                &second_sink,
            )
        })
    };
    let opened = loop {
        let event = next_event(&mut second, "第二份会话快照").await;
        if let ServerMessage::SessionSnapshot { .. } = &event {
            break event;
        }
    };
    assert_eq!(
        match &opened {
            ServerMessage::SessionSnapshot {
                session_id: seen, ..
            } => Some(*seen),
            _ => None,
        },
        Some(session_id),
        "{opened:?}"
    );
    following.await.expect("the snapshot thread panicked");

    let cancelling = {
        let engine = engine.clone();
        let sink = first_sink.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::Cancel {
                    client_msg_id: new_id("cancel"),
                    session_id,
                },
                &sink,
            )
        })
    };
    let first_saw = loop {
        let event = next_event(&mut first, "取消的审计事件").await;
        if let ServerMessage::Audit { action, .. } = &event {
            assert_eq!(action, "cancel");
            break event;
        }
    };
    let second_saw = loop {
        let event = next_event(&mut second, "另一条连接的审计事件").await;
        if let ServerMessage::Audit { action, .. } = &event {
            assert_eq!(action, "cancel");
            break event;
        }
    };
    assert_eq!(
        first_saw, second_saw,
        "the two connections disagree about the same turn"
    );
    cancelling.await.expect("the cancel thread panicked");
}

/// Cancels a session and waits for the two events that cancel broadcasts, ignoring
/// the addressed frames that arrive alongside them.
async fn cancel_and_watch(
    engine: &Engine,
    sink: &chaos_engine::EventSink,
    stream: &mut EventStream,
    session_id: Uuid,
) {
    let engine = engine.clone();
    let sink = sink.clone();
    let cancelling = tokio::task::spawn_blocking(move || {
        dispatch(
            engine,
            ClientMessage::Cancel {
                client_msg_id: new_id("cancel"),
                session_id,
            },
            &sink,
        )
    });
    let mut seen = 0;
    while seen < 2 {
        let event = next_event(stream, "取消产生的事件").await;
        assert!(!matches!(event, ServerMessage::Error { .. }), "{event:?}");
        if matches!(
            event,
            ServerMessage::Cancelled { .. } | ServerMessage::Audit { .. }
        ) {
            seen += 1;
        }
    }
    cancelling.await.expect("the cancel thread panicked");
}

#[tokio::test]
async fn a_connection_that_never_named_the_session_hears_nothing_about_it() {
    let engine = Engine::new();
    let mut owner = engine.subscribe();
    let owner_sink = owner.sink();
    let mut bystander = engine.subscribe();
    let bystander_sink = bystander.sink();

    let opening = {
        let engine = engine.clone();
        let sink = owner_sink.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::CreateSession {
                    client_msg_id: new_id("create"),
                    workspace_id: None,
                },
                &sink,
            )
        })
    };
    let session_id = match next_event(&mut owner, "SessionCreated").await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("expected SessionCreated, got {other:?}"),
    };
    opening.await.expect("the create thread panicked");

    // The owner named the session in its first request after creating it, so what
    // happens in that session comes back to it.
    cancel_and_watch(&engine, &owner_sink, &mut owner, session_id).await;

    // A cancel is a session event on both counts that matter here: it is broadcast
    // rather than addressed, and it names a session. The bystander named nothing, so
    // none of it is its to see -- another tab's conversation is not this tab's data.
    expect_quiet(&mut bystander, "a session this connection never opened").await;

    // Naming the session is all that was missing: the connection was alive the whole
    // time, it simply was not entitled.
    let following = {
        let engine = engine.clone();
        let sink = bystander_sink.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::Snapshot {
                    client_msg_id: new_id("snapshot"),
                    session_id,
                    workspace_id: None,
                },
                &sink,
            )
        })
    };
    loop {
        let event = next_event(&mut bystander, "旁观连接的会话快照").await;
        if matches!(event, ServerMessage::SessionSnapshot { .. }) {
            break;
        }
    }
    following.await.expect("the snapshot thread panicked");
    cancel_and_watch(&engine, &bystander_sink, &mut bystander, session_id).await;
}

#[tokio::test]
async fn a_connection_switched_into_a_session_follows_it_from_then_on() {
    let engine = Engine::new();
    let mut owner = engine.subscribe();
    let owner_sink = owner.sink();
    let mut joiner = engine.subscribe();
    let joiner_sink = joiner.sink();
    let mut stranger = engine.subscribe();

    // A workspace of its own, and in it a session the joiner has never named. Which
    // conversation a `switch_workspace` lands on is the host's decision, so the
    // request that made the move names no session for the routing rule to register.
    engine.handle(ClientMessage::CreateWorkspace {
        client_msg_id: new_id("workspace"),
        name: "切换进来的工作区".into(),
    });
    let (workspace_id, session_id) = created_workspaces(&engine)
        .into_iter()
        .find_map(|workspace| {
            workspace
                .last_session_id
                .map(|session| (workspace.id, session))
        })
        .expect("creating a workspace gives it a session of its own");

    let switching = {
        let engine = engine.clone();
        let sink = joiner_sink.clone();
        tokio::task::spawn_blocking(move || {
            dispatch(
                engine,
                ClientMessage::SwitchWorkspace {
                    client_msg_id: new_id("switch"),
                    workspace_id,
                },
                &sink,
            )
        })
    };
    loop {
        let event = next_event(&mut joiner, "切换后的会话快照").await;
        if let ServerMessage::SessionSnapshot {
            session_id: opened, ..
        } = &event
        {
            assert_eq!(*opened, session_id, "{event:?}");
            break;
        }
    }
    switching.await.expect("the switch thread panicked");

    // Something happens in the conversation the joiner was just shown, and it is not
    // something the joiner caused. Being handed the transcript is what entitles it
    // to hear about the rest.
    cancel_and_watch(&engine, &owner_sink, &mut owner, session_id).await;
    let mut heard = 0;
    while heard < 2 {
        let event = next_event(&mut joiner, "切换进来的连接该听到的事件").await;
        assert!(!matches!(event, ServerMessage::Error { .. }), "{event:?}");
        if matches!(
            event,
            ServerMessage::Cancelled { .. } | ServerMessage::Audit { .. }
        ) {
            heard += 1;
        }
    }

    // A connection that was never handed that conversation is still not entitled to
    // it, switch or no switch.
    expect_quiet_about(
        &mut stranger,
        session_id,
        "a session this connection was never shown",
    )
    .await;
}

/// The workspaces the host has, as it reports them to whoever asks.
fn created_workspaces(engine: &Engine) -> Vec<chaos_engine::WorkspaceInfo> {
    let events = engine.handle(ClientMessage::ListWorkspaces {
        client_msg_id: new_id("list"),
    });
    match events.as_slice() {
        [ServerMessage::Workspaces { workspaces, .. }] => workspaces.clone(),
        other => panic!("unexpected {other:?}"),
    }
}

// --------------------------------------------------------------------------------
// Backpressure
// --------------------------------------------------------------------------------

#[tokio::test]
async fn a_connection_that_stops_reading_is_cut_off_without_losing_a_chunk_it_was_owed() {
    let engine = Engine::new();
    let session_id = create_session(&engine);
    let mut stream = engine.subscribe();
    let sink = stream.sink();

    // The unread connection has to be following the session before anything reaches
    // it, so it names the session once -- the gesture a tab makes when it opens the
    // conversation -- and drops the snapshot that request earns it.
    engine.dispatch_to(
        ClientMessage::Snapshot {
            client_msg_id: new_id("snapshot"),
            session_id,
            workspace_id: None,
        },
        Some(&sink),
        &mut |_event| {},
    );
    let opening = next_event(&mut stream, "the snapshot never reached its own connection").await;
    assert!(
        matches!(opening, ServerMessage::SessionSnapshot { .. }),
        "{opening:?}"
    );

    // `handle` with no subscription publishes to the connections following that
    // session, so this feeds the unread connection without needing a prompt run of
    // its own. Each cancel contributes two session events -- `Cancelled` and
    // `Audit` -- both
    // numbered with the same sequence, which is what makes a hole visible.
    let mut delivered = Vec::new();
    for _ in 0..2_000 {
        engine.handle(ClientMessage::Cancel {
            client_msg_id: new_id("cancel"),
            session_id,
        });
        if !sink.is_live() {
            break;
        }
    }
    assert!(!sink.is_live(), "an unread connection was never cut off");
    while let Some(event) = stream.recv().await {
        delivered.push(event);
    }

    let mut sequences: Vec<u64> = Vec::new();
    for event in &delivered {
        match event {
            ServerMessage::Cancelled { sequence, .. } | ServerMessage::Audit { sequence, .. } => {
                sequences.push(*sequence);
            }
            other => panic!("unexpected broadcast event {other:?}"),
        }
    }
    assert!(
        sequences.len() >= 256,
        "the buffer was shorter than its documented bound"
    );
    assert!(sequences.len().is_multiple_of(2), "{:?}", sequences.len());
    let distinct: Vec<u64> = sequences
        .chunks(2)
        .map(|pair| {
            assert_eq!(
                pair[0], pair[1],
                "a turn's two events were split apart: {pair:?}"
            );
            pair[0]
        })
        .collect();
    let expected: Vec<u64> = (1..=distinct.len() as u64).collect();
    assert_eq!(
        distinct, expected,
        "the delivered events have a hole in them"
    );

    // The engine kept going after the cutoff, so the shortfall is the connection
    // being closed and not the supply of events running out.
    let snapshot = engine.handle(ClientMessage::Snapshot {
        client_msg_id: new_id("snapshot"),
        session_id,
        workspace_id: None,
    });
    let current = match snapshot.as_slice() {
        [ServerMessage::SessionSnapshot { sequence, .. }] => *sequence,
        other => panic!("unexpected {other:?}"),
    };
    assert!(
        current > *sequences.last().expect("nothing was delivered"),
        "the engine stopped producing too, so the cutoff was never exercised"
    );
}

// --------------------------------------------------------------------------------
// The stand-in responder's own pace
// --------------------------------------------------------------------------------

#[tokio::test]
async fn a_paced_stand_in_answer_arrives_over_time() {
    // The stand-in responder is what a host with no provider answers with. Written
    // out in one write, that answer is indistinguishable on screen from a host that
    // cannot stream, so the host is given a say in its pace -- and this is the fact
    // that the pace is supposed to buy: chunks separated in time, and a turn that
    // still ends normally.
    let gap = Duration::from_millis(60);
    let engine = Engine::new().with_demo_pacing(gap);
    let session_id = create_session(&engine);
    let mut stream = engine.subscribe();
    let sink = stream.sink();
    let running = {
        let engine = engine.clone();
        let message = submit(session_id, "把这句话原样慢慢说回来");
        let sink = sink.clone();
        thread::spawn(move || dispatch(engine, message, &sink))
    };

    let ack = next_event(&mut stream, "Ack").await;
    assert!(matches!(ack, ServerMessage::Ack { .. }), "{ack:?}");

    let mut text = String::new();
    let mut chunks = 0_u32;
    let first_at = Instant::now();
    let mut last_at = first_at;
    loop {
        let event = next_event(&mut stream, "演示应答的分块或终态").await;
        match event {
            ServerMessage::TextDelta {
                text: ref piece, ..
            } => {
                text.push_str(piece);
                chunks += 1;
                last_at = Instant::now();
            }
            ServerMessage::Completed { .. } => break,
            other => panic!("the stand-in turn ended oddly: {other:?}"),
        }
    }
    running.join().expect("the prompt thread panicked");

    assert!(text.starts_with("演示响应："), "{text:?}");
    assert!(
        chunks >= 3,
        "the answer came in {chunks} chunk(s), so pacing was never exercised"
    );
    let span = last_at.duration_since(first_at);
    assert!(
        span >= gap * (chunks - 1),
        "{chunks} chunks arrived {span:?} apart in total, shorter than the {gap:?} gap asked for"
    );
}

// --------------------------------------------------------------------------------
// How many answers one process produces at once
// --------------------------------------------------------------------------------

/// A Provider that falls over has to end the turn it was in.
///
/// The alternative is a session that stays busy with nothing producing, and a
/// transcript that stopped mid-sentence with no ending the client can show.
#[tokio::test]
async fn an_adapter_that_panics_ends_the_turn_and_frees_the_session() {
    let engine = Engine::with_adapter(PanicsOnTheFirstAnswer {
        panicked: AtomicBool::new(false),
    });
    let session_id = create_session(&engine);
    let mut stream = engine.subscribe();
    let sink = stream.sink();
    dispatch(engine.clone(), submit(session_id, "崩一次"), &sink);

    let mut text = String::new();
    let mut ending = None;
    while ending.is_none() {
        match next_event(&mut stream, "崩溃这一次的终态").await {
            ServerMessage::TextDelta {
                text: ref piece, ..
            } => text.push_str(piece),
            ServerMessage::Error { code, message } => {
                assert_eq!(code, "agent_failed", "{code}: {message}");
                ending = Some(message);
            }
            ServerMessage::Ack { .. } => continue,
            other => panic!("expected the half answer and then a failure, got {other:?}"),
        }
    }
    assert_eq!(text, "写了一半", "the text already on screen was not kept");
    assert!(
        ending.as_deref().is_some_and(|why| why.contains("终止")),
        "the failure said {ending:?}"
    );

    // The slot and the run registration both came back, which is what a second
    // prompt on the same session can now answer normally.
    dispatch(engine.clone(), submit(session_id, "再来一次"), &sink);
    let mut second = Vec::new();
    loop {
        let event = next_event(&mut stream, "第二次应答").await;
        assert!(
            !matches!(event, ServerMessage::Error { .. }),
            "the session was still owned by the run that panicked: {event:?}"
        );
        let done = matches!(event, ServerMessage::Completed { .. });
        if matches!(event, ServerMessage::TextDelta { .. }) {
            second.push(event);
        }
        if done {
            break;
        }
    }
    // The text only the second answer can produce came through, and the turn ended
    // normally: the panicking run left neither the session nor its registration held.
    assert!(
        second.iter().any(|event| matches!(
            event,
            ServerMessage::TextDelta { text, .. } if text == "这一次写完了"
        )),
        "the second answer never streamed its own text: {second:?}"
    );
}

/// The bound on concurrent answers is real, named, and gives its slots back.
///
/// Turns no longer occupy the connection that asked for them, so the number of
/// threads one process is producing answers on is bounded where the threads are.
#[tokio::test]
async fn a_host_refuses_more_answers_than_it_will_produce_at_once() {
    // More than `MAX_CONCURRENT_RUNS`, without restating the number: the refusal is
    // what matters, and a raised bound only has to stay below this to keep proving.
    const PROMPTS: usize = 64;
    let release = Arc::new(AtomicBool::new(false));
    let engine = Engine::with_adapter(HoldsEveryTurnItIsGiven {
        release: Arc::clone(&release),
    });
    let mut stream = engine.subscribe();
    let sink = stream.sink();
    let sessions = (0..PROMPTS)
        .map(|_| create_session(&engine))
        .collect::<Vec<_>>();
    for session_id in &sessions {
        dispatch(engine.clone(), submit(*session_id, "占住一个名额"), &sink);
    }

    let mut accepted = 0;
    let mut refused = 0;
    while accepted + refused < PROMPTS {
        match next_event(&mut stream, "受理或拒绝").await {
            ServerMessage::Ack { .. } => accepted += 1,
            ServerMessage::Error { code, message } => {
                assert_eq!(code, "too_many_runs", "{code}: {message}");
                assert!(!message.is_empty());
                refused += 1;
            }
            other => panic!("expected an acceptance or a refusal, got {other:?}"),
        }
    }
    assert!(accepted > 0, "nothing was accepted at all");
    assert!(
        refused > 0,
        "{PROMPTS} answers were all accepted, so nothing bounds the threads"
    );

    // Every slot comes back when the answer ends, so a later prompt is not stuck
    // behind a queue nobody drains.
    release.store(true, Ordering::SeqCst);
    let mut settled = 0;
    while settled < accepted {
        match next_event(&mut stream, "被放开的应答的终态").await {
            ServerMessage::Completed { .. } | ServerMessage::Error { .. } => settled += 1,
            ServerMessage::TextDelta { .. } => continue,
            other => panic!("unexpected {other:?}"),
        }
    }
    let spare = create_session(&engine);
    dispatch(engine.clone(), submit(spare, "现在轮到我"), &sink);
    loop {
        match next_event(&mut stream, "释放之后的新应答").await {
            ServerMessage::Completed { .. } => break,
            ServerMessage::TextDelta { .. } | ServerMessage::Ack { .. } => continue,
            other => panic!("a prompt after the runs finished was refused: {other:?}"),
        }
    }
}
