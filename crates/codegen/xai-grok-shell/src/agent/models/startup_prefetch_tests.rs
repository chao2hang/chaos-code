use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::{
    Accept, INFLIGHT, Inflight, State, accept, accept_with_deadline, begin_before_policy_gate,
    clear_for_tests, inject_with_origin_for_tests, lock_or_recover, wait_finished,
};
use crate::agent::config::Config;
use crate::util::config::RemoteSettings;

fn marker_settings() -> Option<RemoteSettings> {
    Some(RemoteSettings {
        path_not_found_hints: Some(true),
        ..RemoteSettings::default()
    })
}

fn registered_marker() -> Option<bool> {
    // Reads the module's own guards, so it reads them the module's way: some
    // tests below hand back a guard poisoned on purpose.
    let inflight = lock_or_recover(&INFLIGHT);
    let cell = inflight.as_ref()?;
    let state = lock_or_recover(&cell.state);
    state.settings.as_ref().and_then(|s| s.path_not_found_hints)
}

/// Same serial group as `init_tests`: both consume the process-wide fetch.
#[test]
#[serial_test::serial(remote_sig_disarm)]
fn begin_does_not_replace_an_inflight_fetch() {
    clear_for_tests();
    super::inject_for_tests(marker_settings());
    begin_before_policy_gate(&Config::default());
    assert_eq!(
        registered_marker(),
        Some(true),
        "the second begin must join the in-flight fetch, not replace it"
    );
    clear_for_tests();
}

#[test]
#[serial_test::serial(remote_sig_disarm)]
fn accept_discards_a_fetch_from_another_origin() {
    clear_for_tests();
    inject_with_origin_for_tests(marker_settings(), "https://elsewhere.invalid".to_string());
    assert!(
        matches!(accept(), Accept::Miss),
        "a fetch from a different origin must not be applied"
    );
    assert!(
        !super::inflight_for_tests(),
        "the rejected fetch must be consumed, not left registered"
    );
}

#[test]
#[serial_test::serial(remote_sig_disarm)]
fn accept_deadline_spends_the_budget() {
    clear_for_tests();
    let never_finishing = Arc::new(Inflight {
        origin: super::resolve_startup_endpoints().proxy_url(),
        state: Mutex::new(State::default()),
        done: Condvar::new(),
    });
    *INFLIGHT.lock().unwrap() = Some(never_finishing);
    assert!(
        matches!(accept_with_deadline(Duration::ZERO), Accept::Consumed(None)),
        "a timed-out wait must spend the budget, not trigger a refetch"
    );
    assert!(
        super::inflight_for_tests(),
        "a timed-out fetch must stay registered so nothing can start behind it"
    );
    clear_for_tests();
}

#[test]
#[serial_test::serial(remote_sig_disarm)]
fn wait_settings_leaves_the_fetch_for_accept() {
    if !crate::util::config::resolve_remote_fetch_enabled() {
        eprintln!("skipped: remote_fetch disabled in this environment");
        return;
    }
    clear_for_tests();
    super::inject_for_tests(marker_settings());
    assert_eq!(
        super::wait_settings(Duration::ZERO).and_then(|s| s.path_not_found_hints),
        Some(true),
    );
    match accept() {
        Accept::Consumed(settings) => assert_eq!(
            settings.and_then(|s| s.path_not_found_hints),
            Some(true),
            "wait_settings must not consume the fetch"
        ),
        Accept::Miss => panic!("wait_settings consumed the fetch"),
    }
}

/// Poisons a guard the way the real failure does it: a thread dies holding it.
/// The panic is caught one frame above the guard, so the process survives and
/// the scope's join makes the poisoning visible before this returns.
fn poison_through_a_panicking_thread<T: Send>(guard: &Mutex<T>) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = guard.lock().unwrap();
                panic!("a worker dying inside the guard");
            }));
        });
    });
    assert!(
        guard.is_poisoned(),
        "the poisoning above must be observable"
    );
}

/// A fetch the worker finished and then died holding the guard over.
fn poisoned_fetch(settings: Option<RemoteSettings>, origin: &str) -> Arc<Inflight> {
    let cell = Arc::new(Inflight {
        origin: origin.to_string(),
        state: Mutex::new(State {
            finished: true,
            panicked: true,
            settings,
            models_write: None,
            settings_write: None,
        }),
        done: Condvar::new(),
    });
    poison_through_a_panicking_thread(&cell.state);
    cell
}

/// `FinishGuard` runs on the worker's way out, often while it is already
/// unwinding. Panicking there is how a lost fetch becomes an aborted process.
#[test]
fn the_finish_guard_finishes_the_fetch_through_a_poisoned_state_guard() {
    let cell = poisoned_fetch(marker_settings(), "https://poisoned.invalid");
    drop(super::FinishGuard(cell.clone()));
    let state = lock_or_recover(&cell.state);
    assert!(
        state.finished,
        "a poisoned guard must not leave the fetch looking still in flight"
    );
    assert!(
        state.settings.is_some(),
        "reaching through the poisoned guard must keep what the worker stored"
    );
}

/// `wait_finished` is how both consumers learn the worker is done. A poisoned
/// guard is an answer about a dead worker, not a reason to panic in the thread
/// that only wanted to boot.
#[test]
fn wait_finished_yields_a_poisoned_guard_instead_of_reading_as_a_timeout() {
    let cell = poisoned_fetch(
        marker_settings(),
        &super::resolve_startup_endpoints().proxy_url(),
    );
    assert!(
        wait_finished(cell, Duration::from_secs(1)).is_some(),
        "a poisoned guard on a finished fetch must not be mistaken for a deadline"
    );
}

/// The failure the wait exists for: the worker dies holding the guard while the
/// boot thread sleeps on it. `Condvar::wait_timeout_while` reports that as a
/// poisoned result, which the waiter must read as an exhausted budget -- the
/// panic this site used to raise came out of a background fetch nobody ordered.
#[test]
fn a_poisoned_guard_under_a_sleeping_waiter_ends_the_wait_without_a_panic() {
    let cell = Arc::new(Inflight {
        origin: super::resolve_startup_endpoints().proxy_url(),
        state: Mutex::new(State::default()),
        done: Condvar::new(),
    });
    poison_through_a_panicking_thread(&cell.state);
    assert!(
        wait_finished(cell, Duration::from_millis(50)).is_none(),
        "a worker that died without notifying must spend the wait, not panic the waiter"
    );
}

/// Both consumers read the registry through this one `Option`. A thread that
/// died holding its guard left a readable value behind, so `accept` must still
/// consume the fetch it finds there, and a later caller must still be able to
/// register and clear one. Whether the settings are then handed out is the
/// egress gate's decision rather than this module's, so the expectation follows
/// the same knob the shipped code reads instead of skipping the test.
#[test]
#[serial_test::serial(remote_sig_disarm)]
fn accept_consumes_a_fetch_behind_a_poisoned_registry_guard() {
    let egress_open = crate::util::config::resolve_remote_fetch_enabled();
    clear_for_tests();
    super::inject_for_tests(marker_settings());
    poison_through_a_panicking_thread(&INFLIGHT);
    match accept() {
        Accept::Consumed(settings) => {
            assert!(
                egress_open,
                "settings cannot be handed out with remote_fetch off"
            );
            assert_eq!(
                settings.and_then(|s| s.path_not_found_hints),
                Some(true),
                "the fetch registered before the poisoning must still be delivered"
            );
        }
        Accept::Miss => assert!(
            !egress_open,
            "a poisoned registry guard must not turn an open egress gate into a miss"
        ),
    }
    assert!(
        !super::inflight_for_tests(),
        "the consumed fetch must be deregistered through the poisoned guard too"
    );
    super::inject_with_origin_for_tests(marker_settings(), "https://poisoned.invalid".to_string());
    assert!(
        super::inflight_for_tests(),
        "registering a fetch must still work through the poisoned guard"
    );
    clear_for_tests();
    assert!(
        !super::inflight_for_tests(),
        "clearing the registry must still work through the poisoned guard"
    );
    // The rest of this serial group reads the registry with `.unwrap()`.
    INFLIGHT.clear_poison();
}

/// The commit point reads two guards: the registry, then the worker's own state.
/// By the time it reaches the second one the registry entry is already gone, so
/// a panic there would lose the settings with nothing left to retry from -- and
/// this read happens before the egress gate is consulted, which is why the test
/// asserts the deregistration rather than only the returned settings.
#[test]
#[serial_test::serial(remote_sig_disarm)]
fn accept_consumes_a_fetch_behind_a_poisoned_state_guard() {
    let egress_open = crate::util::config::resolve_remote_fetch_enabled();
    clear_for_tests();
    let cell = poisoned_fetch(
        marker_settings(),
        &super::resolve_startup_endpoints().proxy_url(),
    );
    *lock_or_recover(&INFLIGHT) = Some(cell);
    match accept() {
        Accept::Consumed(settings) => {
            assert!(
                egress_open,
                "settings cannot be handed out with remote_fetch off"
            );
            assert_eq!(
                settings.and_then(|s| s.path_not_found_hints),
                Some(true),
                "the settings stored before the poisoning must survive the commit"
            );
        }
        Accept::Miss => assert!(
            !egress_open,
            "a poisoned state guard must not turn an open egress gate into a miss"
        ),
    }
    assert!(
        !super::inflight_for_tests(),
        "the commit must reach its way through the poisoned state guard to deregister the fetch"
    );
    clear_for_tests();
}

/// `wait_settings` is the read-only peek `bootstrap` takes before the gate. Its
/// registry read comes before the egress check, so a poisoned registry guard
/// must still answer with the fetch instead of with "nothing in flight".
#[test]
#[serial_test::serial(remote_sig_disarm)]
fn wait_settings_finds_the_fetch_through_a_poisoned_registry_guard() {
    let egress_open = crate::util::config::resolve_remote_fetch_enabled();
    clear_for_tests();
    super::inject_for_tests(marker_settings());
    poison_through_a_panicking_thread(&INFLIGHT);
    let peek = || super::wait_settings(Duration::ZERO).and_then(|s| s.path_not_found_hints);
    if egress_open {
        assert_eq!(
            peek(),
            Some(true),
            "a poisoned registry guard must not read as an absent fetch"
        );
    } else {
        assert_eq!(peek(), None, "the egress gate still declines to hand out");
        assert!(
            super::inflight_for_tests(),
            "a declined peek must leave the fetch registered for `accept`"
        );
    }
    clear_for_tests();
    INFLIGHT.clear_poison();
}

/// The pre-gate `begin` answers "is something already in flight" from the same
/// poisoned guard; that answer is what keeps a second fetch from starting behind
/// a dead one.
#[test]
#[serial_test::serial(remote_sig_disarm)]
fn begin_reports_the_in_flight_fetch_through_a_poisoned_registry_guard() {
    clear_for_tests();
    super::inject_for_tests(marker_settings());
    poison_through_a_panicking_thread(&INFLIGHT);
    assert!(
        begin_before_policy_gate(&Config::default()),
        "a poisoned registry guard must not read as 'nothing in flight'"
    );
    assert_eq!(
        registered_marker(),
        Some(true),
        "the fetch behind the poisoned guard must still be the registered one"
    );
    clear_for_tests();
    INFLIGHT.clear_poison();
}
