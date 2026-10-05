//! Locking that reads a guard instead of panicking on how its last holder died.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// `Mutex::lock`, without the second panic.
///
/// A mutex reports poisoning when its holder died with the lock held. The
/// `unwrap`/`expect` form then raises a fresh panic in whichever thread only
/// wanted to read a value -- a thread that has no idea a worker died and no way
/// to report it. Every guard reached through this trait holds a plain value (an
/// `Option`, a counter, a small struct of those) whose invariant does not
/// depend on the dead holder finishing its write, and every consumer already
/// has to cope with a value left half-updated, because a `Drop` or an early
/// `return` on a panic-free path can leave that behind too.
///
/// The panicking form is not merely noisy here. `Drop` implementations take
/// these locks (`BlockingWaitGuard::drop` in `tools::tool_context`,
/// `TurnSubagentScopeGuard::drop` and the prefetch module's `FinishGuard::drop`),
/// a `Drop` running during unwinding is exactly the path a poisoning panic comes
/// down, and a panic raised while already unwinding aborts the process.
///
/// Recovery returns the data and leaves the mutex poisoned, so a `.unwrap()`
/// elsewhere still reports that a holder died. `parking_lot` locks and atomics
/// have no poisoning and are not this trait's business.
pub(crate) trait LockOrRecover<T> {
    /// Takes the lock, reading whatever the dead holder left behind.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockOrRecover<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `RwLock::read` and `RwLock::write`, without the second panic.
///
/// Same reasoning as [`LockOrRecover`]. The poisoning can only come from a
/// writer, so `read` fails alongside it, and what the dead writer left is the
/// same structurally valid value either way.
pub(crate) trait ReadWriteOrRecover<T> {
    /// Takes the read lock, reading whatever a dead writer left behind.
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T>;

    /// Takes the write lock, updating whatever a dead writer left behind.
    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T> ReadWriteOrRecover<T> for RwLock<T> {
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Poisons a mutex the way the real failure does: a thread dies holding it, and
/// the panic is caught one frame above the guard so the process survives. The
/// scope's join makes the poisoning visible before this returns, and the assert
/// is what stops a caller from measuring recovery on a guard that was never
/// poisoned at all.
#[cfg(test)]
pub(crate) fn poison_mutex_through_a_panicking_thread<T: Send>(guard: &Mutex<T>) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = guard.lock().unwrap();
                panic!("a holder dying inside the guard");
            }));
        });
    });
    assert!(
        guard.is_poisoned(),
        "the poisoning above must be observable, or the caller proves nothing"
    );
}

/// The same for a write lock, which is the only way an `RwLock` gets poisoned.
#[cfg(test)]
pub(crate) fn poison_rwlock_through_a_panicking_writer<T: Send + Sync>(guard: &RwLock<T>) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = guard.write().unwrap();
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
#[path = "shared_guard_tests.rs"]
mod tests;
