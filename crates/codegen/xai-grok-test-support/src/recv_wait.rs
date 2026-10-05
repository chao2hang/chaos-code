//! Bounded `recv()` for tests: a wait for an event that never arrives has to fail the
//! test, not the CI job.
//!
//! The only ceiling on a CI job is its `timeout-minutes`. A bare `rx.recv().await` in a
//! test therefore has no failure mode: when the event does not come, the test neither
//! passes nor fails, it keeps the job alive until the runner kills it, and the log says
//! nothing for the whole budget. `xai-grok-shell`'s workflow-manager suite lost a whole
//! job that way -- the last completed test is timestamped 04:00:01, libtest's own
//! watchdog names the hanging test at 04:00:55, and the runner reports `cancelled` at
//! 04:30:09 with no failing test anywhere in the log.
//!
//! Every receiver a test waits on gets the same two things here: a deadline, and a
//! message that names the awaited event. `what` is the phrase the test used to hand to
//! `.expect(..)`, so the failure text keeps what the unbounded version carried and adds
//! which wait ran out.
//!
//! The deadline is short and uniform on purpose. A test that expects an event from a task
//! it spawned in the same `#[tokio::test]` body gets it within a few milliseconds, so 2 s
//! is already two orders of magnitude of headroom; it is also the value
//! `session/workflow/manager.rs` had been using with no reported flake. A wait that
//! legitimately needs longer passes its own budget to [`within`] at the call site instead
//! of raising the shared default.
//!
//! # Paused clocks
//!
//! Under `#[tokio::test(start_paused = true)]` the runtime is not slow, it is *fast*: when
//! every task is pending, tokio advances the clock to the shortest timer that is armed. A
//! bound is just another timer, so a 2 s default armed next to a production timer of 30 s
//! wins the race by being smaller, the deadline fires at t=2s of the test's own clock, and
//! the test reports a phantom timeout for a production path that was merely doing what it
//! is designed to do. The bound has to sit *above* the production timer the event is
//! emitted behind, expressed in the same units as that timer, e.g.
//! `within_option_budget(10 * PARENT_ACK_TIMEOUT, ..)`. Real time is unaffected: the
//! budgets here only bound waits that would otherwise hang the job.

use std::future::Future;
use std::time::Duration;

/// Default ceiling for a test-side event wait. See the module docs for why it is short.
pub const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(2);

/// Runs `wait` under an explicit budget, panicking with `what` if it does not finish.
///
/// Takes the wait rather than a receiver so a test can bound a wait that is not a
/// `recv()` -- an `rx.changed()`, a `JoinHandle`, a helper call -- without inventing a
/// second idiom. [`crate::scaled`] is applied, so a CI lane that raises
/// `GROK_TEST_TIMEOUT_SCALE` slows tests down instead of failing them.
pub async fn within<T>(budget: Duration, wait: impl Future<Output = T>, what: &str) -> T {
    let budget = crate::scaled(budget);
    match tokio::time::timeout(budget, wait).await {
        Ok(v) => v,
        Err(_) => hit_deadline(what, budget),
    }
}

/// [`within`] under [`DEFAULT_RECV_TIMEOUT`].
pub async fn within_default<T>(wait: impl Future<Output = T>, what: &str) -> T {
    within(DEFAULT_RECV_TIMEOUT, wait, what).await
}

/// Bounds a wait that yields `Option<T>`, so a test gets one call for both ways the event
/// can fail to appear: the deadline runs out, or the channel closes first.
///
/// This is the form for a receiver that is not one of the tokio channels -- a harness type
/// with its own `recv()` over a `Notify`, say -- where [`RecvBounded`] cannot be implemented
/// without making test-support a real dependency of the crate that owns the type.
pub async fn within_option<T>(wait: impl Future<Output = Option<T>>, what: &str) -> T {
    within_option_budget(DEFAULT_RECV_TIMEOUT, wait, what).await
}

/// [`within_option`] under a budget the caller picks, for the one case the default
/// cannot cover: a wait on a paused clock behind a production timer, where the
/// default bound would fire first. See the module docs.
pub async fn within_option_budget<T>(
    budget: Duration,
    wait: impl Future<Output = Option<T>>,
    what: &str,
) -> T {
    match within(budget, wait, what).await {
        Some(v) => v,
        None => closed_channel(what),
    }
}

/// Waits on a `oneshot` without an unbounded `await`.
///
/// A free function rather than another [`RecvBounded`] impl because `oneshot::Receiver`
/// yields its value by consuming itself, which a `&mut self` trait method cannot do. It is
/// itself a future, so it goes straight to [`within`].
pub async fn oneshot<T>(rx: tokio::sync::oneshot::Receiver<T>, what: &str) -> T {
    match within_default(rx, what).await {
        Ok(v) => v,
        Err(e) => panic!("{what}: oneshot closed ({e}) before any event"),
    }
}

/// The one place the deadline failure text is written, so every wait in the workspace
/// reports a ran-out budget in the same shape.
fn hit_deadline(what: &str, budget: Duration) -> ! {
    panic!("{what}: no event within {}", secs(budget))
}

/// The one place a closed `mpsc` reports itself. The unbounded `.expect(..)` form the
/// converted call sites used to write surfaced this as whatever phrase the author had
/// typed, which for most of them read like a timeout.
fn closed_channel(what: &str) -> ! {
    panic!("{what}: channel closed before any event")
}

/// Renders a budget as `2s` or `50ms` rather than `2s` / `2.000000000s`, so the panic text
/// a test asserts on is the text a human would write.
fn secs(d: Duration) -> String {
    if d.subsec_millis() == 0 {
        format!("{}s", d.as_secs())
    } else if d.subsec_nanos().is_multiple_of(1_000_000) {
        format!("{}ms", d.as_millis())
    } else {
        format!("{d:?}")
    }
}

/// A receiver a test may wait on without an unbounded `await`.
///
/// Implemented per receiver type rather than as one free function because the tokio
/// channels disagree about what a closed channel means: `mpsc` yields `None`, `broadcast`
/// yields `Err`. A closed channel is a failed test either way, and both say so with `what`
/// -- the unbounded `.expect(..)` form collapsed that difference into whichever phrase the
/// author happened to write. `oneshot` consumes its receiver and so has [`oneshot`]
/// instead.
///
/// The returned future is spelled out rather than written `async fn` so callers can hold it
/// across `tokio::spawn`; an `async fn` in a public trait erases the auto traits that make
/// that possible.
pub trait RecvBounded {
    /// The value the receiver yields on the happy path.
    type Item;

    /// Waits for one item under [`DEFAULT_RECV_TIMEOUT`], naming `what` in every failure.
    fn recv_bounded(&mut self, what: &str) -> impl Future<Output = Self::Item> + Send;
}

impl<T: Send> RecvBounded for tokio::sync::mpsc::Receiver<T> {
    type Item = T;
    fn recv_bounded(&mut self, what: &str) -> impl Future<Output = T> + Send {
        let budget = crate::scaled(DEFAULT_RECV_TIMEOUT);
        async move {
            match tokio::time::timeout(budget, self.recv()).await {
                Ok(Some(v)) => v,
                Ok(None) => closed_channel(what),
                Err(_) => hit_deadline(what, budget),
            }
        }
    }
}

impl<T: Send> RecvBounded for tokio::sync::mpsc::UnboundedReceiver<T> {
    type Item = T;
    fn recv_bounded(&mut self, what: &str) -> impl Future<Output = T> + Send {
        let budget = crate::scaled(DEFAULT_RECV_TIMEOUT);
        async move {
            match tokio::time::timeout(budget, self.recv()).await {
                Ok(Some(v)) => v,
                Ok(None) => closed_channel(what),
                Err(_) => hit_deadline(what, budget),
            }
        }
    }
}

impl<T: Clone + Send> RecvBounded for tokio::sync::broadcast::Receiver<T> {
    type Item = T;
    fn recv_bounded(&mut self, what: &str) -> impl Future<Output = T> + Send {
        let budget = crate::scaled(DEFAULT_RECV_TIMEOUT);
        async move {
            match tokio::time::timeout(budget, self.recv()).await {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => panic!("{what}: broadcast unavailable ({e})"),
                Err(_) => hit_deadline(what, budget),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn buffered_mpsc_yields_the_event() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(4);
        tx.send(7).await.unwrap();
        assert_eq!(rx.recv_bounded("a number").await, 7);
    }

    #[tokio::test]
    async fn unbounded_mpsc_yields_the_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tx.send("hi".into()).unwrap();
        assert_eq!(rx.recv_bounded("a greeting").await, "hi");
    }

    #[tokio::test]
    async fn oneshot_yields_the_event() {
        let (tx, rx) = tokio::sync::oneshot::channel::<u8>();
        tx.send(3).unwrap();
        assert_eq!(oneshot(rx, "a reply").await, 3);
    }

    #[tokio::test]
    async fn broadcast_yields_the_event() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<u8>(4);
        tx.send(9).unwrap();
        assert_eq!(rx.recv_bounded("a fanout").await, 9);
    }

    /// The wait has to stay `Send`, because a converted call site is free to hold it across
    /// `tokio::spawn`; an `async fn` in the trait would have quietly removed that option.
    #[tokio::test]
    async fn the_wait_can_be_moved_into_another_task() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        let task = tokio::spawn(async move { rx.recv_bounded("a spawned wait").await });
        tx.send(11).unwrap();
        assert_eq!(task.await.unwrap(), 11);
    }

    /// The `Option`-yielding form is for a receiver the trait cannot cover, so it has to
    /// carry the same three outcomes: value, closed, deadline.
    #[tokio::test]
    async fn within_option_yields_the_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        tx.send(5).unwrap();
        assert_eq!(within_option(rx.recv(), "a harness event").await, 5);
    }

    #[tokio::test]
    #[should_panic(expected = "a harness event: channel closed before any event")]
    async fn within_option_names_a_closed_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        drop(tx);
        within_option(rx.recv(), "a harness event").await;
    }

    #[tokio::test]
    #[should_panic(expected = "a harness event that never comes: no event within 2s")]
    async fn within_option_names_a_ran_out_deadline() {
        let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        within_option(rx.recv(), "a harness event that never comes").await;
    }

    /// The reason the module exists: a wait that never resolves has to fail the test
    /// inside the deadline. Nothing is ever sent, so the only way this passes is the panic
    /// carrying the deadline text and the name of the awaited event.
    #[tokio::test]
    #[should_panic(expected = "an event that never comes: no event within 2s")]
    async fn a_wait_that_never_resolves_names_itself_inside_the_deadline() {
        let (_tx, mut rx) = tokio::sync::mpsc::channel::<u32>(4);
        rx.recv_bounded("an event that never comes").await;
    }

    /// A dropped sender used to surface as whatever phrase the author had passed to
    /// `.expect(..)`, which for most of the converted call sites read like a timeout. It
    /// now says the channel closed, and still names the wait.
    #[tokio::test]
    #[should_panic(expected = "the handoff: channel closed before any event")]
    async fn a_closed_mpsc_names_the_wait_and_the_cause() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(4);
        drop(tx);
        rx.recv_bounded("the handoff").await;
    }

    #[tokio::test]
    #[should_panic(expected = "the ack: oneshot closed")]
    async fn a_closed_oneshot_names_the_wait_and_the_cause() {
        let (tx, rx) = tokio::sync::oneshot::channel::<u32>();
        drop(tx);
        oneshot(rx, "the ack").await;
    }

    #[tokio::test]
    #[should_panic(expected = "a lagged read: broadcast unavailable")]
    async fn a_lagged_broadcast_names_the_wait_and_the_cause() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<u8>(1);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        rx.recv_bounded("a lagged read").await;
    }

    /// A wait that legitimately needs longer says so at the call site; the shared default
    /// stays short for everyone else. 50 ms is below the default, so this can only pass if
    /// the explicit budget is the one in force.
    #[tokio::test]
    #[should_panic(expected = "a short budget: no event within 50ms")]
    async fn an_explicit_budget_replaces_the_default() {
        let (_tx, mut rx) = tokio::sync::mpsc::channel::<u32>(4);
        let got: Option<u32> = within(Duration::from_millis(50), rx.recv(), "a short budget").await;
        got.unwrap();
    }

    /// Stands in for a production ack timeout the test has to wait behind, e.g. the 30 s
    /// `PARENT_ACK_TIMEOUT` in `xai-grok-shell`'s subagent usage fold.
    const PRODUCTION_ACK_TIMEOUT: Duration = Duration::from_secs(30);

    /// The trap the default budget walks into on a paused clock: tokio advances to the
    /// shortest armed timer, so a 2 s bound fires at t=2s while the producer is still
    /// inside its 30 s timer, and the test blames production for a wait that was fine.
    /// The companion test below is the fix; together they pin the rule.
    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "fallback event: no event within 2s")]
    async fn the_default_bound_preempts_a_production_timer_on_a_paused_clock() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        tokio::spawn(async move {
            tokio::time::sleep(PRODUCTION_ACK_TIMEOUT).await;
            let _ = tx.send(1);
        });
        rx.recv_bounded("fallback event").await;
    }

    /// Same paused clock, same producer, same cost in real time: a budget above the
    /// production timer lets the clock reach the send instead of the bound.
    #[tokio::test(start_paused = true)]
    async fn a_budget_above_the_production_timer_sees_it_on_a_paused_clock() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        tokio::spawn(async move {
            tokio::time::sleep(PRODUCTION_ACK_TIMEOUT).await;
            let _ = tx.send(1);
        });
        let got =
            within_option_budget(10 * PRODUCTION_ACK_TIMEOUT, rx.recv(), "fallback event").await;
        assert_eq!(got, 1);
    }

    /// The budgeted `Option` form keeps the closed-channel wording, which is the whole
    /// reason the `Option` variants exist.
    #[tokio::test]
    #[should_panic(expected = "the closed handoff: channel closed before any event")]
    async fn a_budgeted_option_wait_names_a_closed_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        drop(tx);
        within_option_budget(Duration::from_secs(30), rx.recv(), "the closed handoff").await;
    }

    /// The name has to survive into the message when a test waits twice: the panic text is
    /// the only thing telling a reader which of the two waits ran out. The second sender is
    /// held without sending, so only the deadline can end this test.
    #[tokio::test]
    #[should_panic(expected = "second wait: no event within 2s")]
    async fn the_name_distinguishes_two_waits_in_one_test() {
        let (tx, mut first) = tokio::sync::mpsc::channel::<u32>(4);
        let (_held, mut second) = tokio::sync::mpsc::channel::<u32>(4);
        tx.send(1).await.unwrap();
        assert_eq!(first.recv_bounded("first wait").await, 1);
        second.recv_bounded("second wait").await;
    }

    /// `GROK_TEST_TIMEOUT_SCALE` has to reach this deadline the way it reaches every other
    /// harness timeout, or a loaded CI lane fails instead of slowing. Asserted from child
    /// processes because [`crate::scaled`] reads the real environment, and a second
    /// in-process test would only prove the variable was set, not that the deadline moved.
    #[test]
    fn the_scale_factor_reaches_the_deadline() {
        let unscaled = run_worker("1");
        assert!(
            unscaled.contains("1 failed")
                && unscaled.contains("a scaled budget: no event within 60ms"),
            "at scale 1 the 60ms budget must miss the 90ms send, got: {unscaled}"
        );

        let doubled = run_worker("2");
        assert!(
            doubled.contains("1 passed"),
            "scale 2 should turn the same worker green, got: {doubled}"
        );
    }

    /// Re-runs this test binary for [`scaled_deadline_worker`] at a given scale and returns
    /// everything it printed. Panic text lands on stderr, the libtest summary on stdout, and
    /// the assertion needs both.
    fn run_worker(scale: &str) -> String {
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args([
                "--exact",
                "recv_wait::tests::scaled_deadline_worker",
                "--ignored",
                "--nocapture",
            ])
            .env("GROK_TEST_TIMEOUT_SCALE", scale)
            .env("CHAOS_RECV_WAIT_WORKER", "1")
            .output()
            .expect("re-run this test binary");
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        text
    }

    /// Driven by [`the_scale_factor_reaches_the_deadline`]: a 60 ms budget against a 90 ms
    /// send is a miss at scale 1 and a hit at scale 2.
    #[tokio::test]
    #[ignore = "driven by the_scale_factor_reaches_the_deadline with the scale set per run"]
    async fn scaled_deadline_worker() {
        assert_eq!(
            std::env::var("CHAOS_RECV_WAIT_WORKER").as_deref(),
            Ok("1"),
            "this test only runs as a worker of the_scale_factor_reaches_the_deadline"
        );
        let (tx, rx) = tokio::sync::oneshot::channel::<u8>();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(90)).await;
            let _ = tx.send(1);
        });
        let got = within(Duration::from_millis(60), rx, "a scaled budget").await;
        assert_eq!(
            got.expect("the scaled budget should cover the 90ms send"),
            1
        );
    }
}
