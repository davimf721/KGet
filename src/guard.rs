//! Panic containment for library code.
//!
//! KGet runs inside long-lived hosts (the daemon, the MCP server, apps that
//! embed `kget-ffi`), so a bug that panics in one download must not take the
//! rest of the process down with it.
//!
//! **Poisoned locks.** A `Mutex` is poisoned when a thread panics while
//! holding it. Every lock in the library guards plain bookkeeping (job
//! tables, byte counters, ranges, writers) that is consistent between
//! statements, so the policy is to recover the guard with [`lock`] (or
//! `unwrap_or_else(PoisonError::into_inner)`) instead of spreading the panic
//! to every thread that touches the lock afterwards.
//!
//! **Thread boundaries.** Threads the library spawns run their body through
//! [`catch`] (or `catch_unwind`) and turn a panic into an error, a failed
//! job or a log line, and release what they hold (slots, counters) either way.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `m`, recovering the guard if a panicking thread poisoned it.
pub(crate) fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The message of a panic payload (`panic!` with a literal or a format).
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Run `f`, turning a panic into `Err(message)`.
///
/// `AssertUnwindSafe` is sound here because callers either drop everything
/// `f` touched or only share it through [`lock`]-style recovering locks.
pub(crate) fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| panic_message(&*p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_reports_the_panic_message() {
        assert_eq!(catch(|| 7), Ok(7));
        assert_eq!(catch(|| panic!("boom")), Err::<(), _>("boom".into()));
        let n = 3;
        assert_eq!(catch(|| panic!("n = {n}")), Err::<(), _>("n = 3".into()));
    }

    #[test]
    fn lock_recovers_from_poisoning() {
        let m = std::sync::Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison it");
        })
        .join();
        assert!(m.is_poisoned());
        *lock(&m) += 1;
        assert_eq!(*lock(&m), 2);
    }
}
