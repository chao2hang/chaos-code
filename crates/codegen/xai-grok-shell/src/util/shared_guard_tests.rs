//! The recovery is only correct if the guard really was poisoned and the value
//! is really the one the dead holder left. Both are asserted here; the consumer
//! modules drive their own shipped paths with the same poison helpers.

use super::*;

#[test]
fn a_poisoned_mutex_still_answers_with_what_the_dead_holder_stored() {
    let guard = Mutex::new(Some("stored before the panic"));
    poison_mutex_through_a_panicking_thread(&guard);
    assert_eq!(
        guard.lock_or_recover().as_deref(),
        Some("stored before the panic"),
        "recovering must read the value, not a default"
    );
    assert!(
        guard.is_poisoned(),
        "recovery must not silently clear the poisoning: another module's \
         `.unwrap()` still has to see that a holder died"
    );
}

#[test]
fn a_poisoned_mutex_can_still_be_written() {
    let guard = Mutex::new(0_u32);
    poison_mutex_through_a_panicking_thread(&guard);
    *guard.lock_or_recover() = 7;
    assert_eq!(
        *guard.lock_or_recover(),
        7,
        "the write must land, and the guard must still answer afterwards"
    );
}

#[test]
fn a_poisoned_rwlock_answers_on_both_sides() {
    let guard = RwLock::new(Some(true));
    poison_rwlock_through_a_panicking_writer(&guard);
    assert_eq!(*guard.read_or_recover(), Some(true));
    *guard.write_or_recover() = Some(false);
    assert_eq!(
        *guard.read_or_recover(),
        Some(false),
        "the write must land, and the lock must still answer afterwards"
    );
    assert!(guard.is_poisoned(), "and the poisoning must stay visible");
}

/// The panicking accessor is the whole point of the poisoning flag: it stays
/// set across a recovery, so code that has not opted into recovery still hears
/// about the dead holder. Asserting that here is what stops a later "tidy up"
/// from swapping in `clear_poison()`.
#[test]
fn recovery_leaves_the_panicking_accessor_still_reporting_the_dead_holder() {
    let guard = Mutex::new(0_u32);
    poison_mutex_through_a_panicking_thread(&guard);
    drop(guard.lock_or_recover());
    let refused = guard
        .lock()
        .expect_err("a poisoned mutex must still refuse");
    assert_eq!(
        *refused.into_inner(),
        0,
        "and hand back the value it refused to hand over"
    );
    let rw = RwLock::new(0_u32);
    poison_rwlock_through_a_panicking_writer(&rw);
    drop(rw.read_or_recover());
    assert!(
        rw.read().is_err(),
        "an RwLock poisoned by a writer must refuse readers too"
    );
}

#[test]
fn an_unpoisoned_guard_answers_and_stays_unpoisoned() {
    let guard = Mutex::new(1_u32);
    *guard.lock_or_recover() = 2;
    assert!(!guard.is_poisoned());
    let rw = RwLock::new(3_u32);
    assert_eq!(*rw.read_or_recover(), 3);
    assert!(
        !rw.is_poisoned(),
        "recovery must not poison anything itself"
    );
}
