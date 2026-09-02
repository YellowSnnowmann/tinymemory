//! Serialises the tests that reach `tinymemory_core`'s process-global seams.
//!
//! The seams — event sink, error reporter, NLP host, scheduler gate and
//! shutdown host — are `static`s owned by the process, not by whoever installs
//! them. `libtest` runs a binary's tests on parallel threads, so two tests that
//! each install a seam are racing: one's `set_scheduler_gate` or
//! `set_manual_override` lands between the other's write and its assertion, and
//! the assertion then fails for reasons that have nothing to do with the test
//! that reported it.
//!
//! [`crate::host_test::HostSeamsRestore`] does not solve this on its own, and it
//! is worth saying why, because it looks as though it should. It restores what a
//! test found, which fixes *ordering* — a later test cannot inherit an earlier
//! one's gate. It says nothing about two tests running at the same time.
//!
//! A test that installs or reads a global seam must hold this for its whole
//! body, taken before it captures anything. See issue #130.
//!
//! The lock is `tokio`'s rather than the standard library's for one reason: two
//! of the three callers are `#[tokio::test]` and hold it across `.await`, which
//! is exactly what `clippy::await_holding_lock` exists to stop you doing with a
//! `std` guard. Hence the pair of accessors below — the sync one is not an
//! alternative to the async one, it is for the caller that has no runtime.

use std::sync::OnceLock;

use tokio::sync::{Mutex, MutexGuard};

static SEAMS: OnceLock<Mutex<()>> = OnceLock::new();

fn seams() -> &'static Mutex<()> {
    SEAMS.get_or_init(|| Mutex::new(()))
}

/// Blocks the current thread until no other test holds the global seams.
///
/// # Panics
///
/// Panics if called from inside a tokio runtime — use [`hold_global_seams_async`]
/// there. That is `blocking_lock`'s own rule, and it is the right failure: a
/// blocking wait on a runtime thread is a deadlock waiting for the right
/// scheduling.
pub(crate) fn hold_global_seams() -> MutexGuard<'static, ()> {
    seams().blocking_lock()
}

/// The same lock, awaited, for a test that runs on a tokio runtime.
pub(crate) async fn hold_global_seams_async() -> MutexGuard<'static, ()> {
    seams().lock().await
}

/// The lock is actually exclusive.
///
/// Worth pinning, because everything above only helps if this holds, and a
/// refactor could quietly make it a no-op — a second `OnceLock`, a guard
/// dropped at the end of its own statement rather than the test body — without
/// any test noticing. The failure it guards against is invisible by nature:
/// the suite would simply go back to being flaky somewhere else.
#[test]
fn only_one_thread_holds_the_seams_at_a_time() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static INSIDE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..200 {
                    let _seams = hold_global_seams();
                    let now = INSIDE.fetch_add(1, Ordering::SeqCst) + 1;
                    PEAK.fetch_max(now, Ordering::SeqCst);
                    std::thread::yield_now();
                    INSIDE.fetch_sub(1, Ordering::SeqCst);
                }
            });
        }
    });

    assert_eq!(
        PEAK.load(Ordering::SeqCst),
        1,
        "two threads held the global seams at once, so serialising the tests \
         that install them buys nothing"
    );
}
