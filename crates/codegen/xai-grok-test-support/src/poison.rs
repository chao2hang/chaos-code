//! Poisons a lock the way the real failure does, so a test can prove what its crate does next.
//!
//! `std::sync` locks report poisoning when their holder panicked with the lock held. Two
//! spellings treat that report as news they do not want: `unwrap` raises a second panic in a
//! thread that only wanted to read a value, and `.ok()` throws the report away and leaves the
//! caller to guess -- the same `None` whether the lock was poisoned, the value was never set, or
//! the read never happened. Both are ratcheted against (`scripts/ci/panic-site-census.py` and
//! `scripts/ci/check-lock-poison.py`).
//!
//! A recovery path cannot be tested by writing `PoisonError` by hand: it is only constructible
//! around a real guard. The only faithful setup is the real one -- a thread dies holding the
//! lock, with the panic caught one frame above the guard so the process survives. These helpers
//! do exactly that and then assert the lock really is poisoned, which stops a caller from
//! measuring recovery on a lock that was never poisoned at all and passing anyway.
//!
//! A poisoned lock stays poisoned, and these take locks by reference, so a test that poisons a
//! process-global lock has to leave the value it cares about in place afterwards: sibling tests
//! in the same binary read the same static, and a recovering reader must answer identically
//! whether or not some earlier test poisoned it.

use std::sync::{Mutex, RwLock};

/// Kills a thread holding `guard`'s mutex, leaving `guard` poisoned and the process alive.
pub fn mutex_through_a_panicking_thread<T: Send>(guard: &Mutex<T>) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Recovering here keeps the only panic in this module the deliberate one below.
                let _held = guard
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                panic!("a holder dying inside the guard");
            }));
        });
    });
    assert!(
        guard.is_poisoned(),
        "the poisoning above must be observable, or the caller proves nothing"
    );
}

/// Kills a thread holding `guard`'s write lock, leaving `guard` poisoned and the process alive.
///
/// A writer is the only way an `RwLock` gets poisoned, so there is no matching reader helper.
pub fn rwlock_through_a_panicking_writer<T: Send + Sync>(guard: &RwLock<T>) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = guard
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                panic!("a writer dying inside the guard");
            }));
        });
    });
    assert!(
        guard.is_poisoned(),
        "the poisoning above must be observable, or the caller proves nothing"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The helper's own contract: the lock is poisoned, the value survives, and the caller's
    /// process is still alive to observe it.
    #[test]
    fn poisoning_leaves_the_value_and_the_process_alone() {
        let guard = Mutex::new(7u32);
        mutex_through_a_panicking_thread(&guard);
        assert!(guard.is_poisoned());
        let recovered = guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(*recovered, 7, "the dead holder never reached its write");
    }

    /// A poisoned `RwLock` refuses `.write().unwrap()` but yields its data through recovery, for
    /// readers as well as writers.
    #[test]
    fn a_poisoned_rwlock_still_yields_its_data_to_both_sides() {
        let guard = RwLock::new(String::from("cached"));
        rwlock_through_a_panicking_writer(&guard);
        assert!(guard.is_poisoned());
        let read = guard
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(read.as_str(), "cached");
        drop(read);
        let mut write = guard
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write.push_str("-updated");
    }

    /// The assert inside the helper is load-bearing: a lock that is already poisoned by an
    /// earlier panic still satisfies it, so it cannot silently no-op on a fresh lock either.
    #[test]
    fn poisoning_a_fresh_lock_is_what_the_helper_claims() {
        let guard = Mutex::new(());
        assert!(
            !guard.is_poisoned(),
            "a fresh lock must start unpoisoned for this to mean anything"
        );
        mutex_through_a_panicking_thread(&guard);
        assert!(guard.is_poisoned());
        assert!(
            std::panic::catch_unwind(|| guard.lock().unwrap()).is_err(),
            "the panicking spelling must still panic, so recovery is a choice and not the default"
        );
    }
}
