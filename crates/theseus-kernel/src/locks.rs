//! One writer at a time per execution (theseus-id9).
//!
//! A kernel transition reads an execution (or one of its actions), changes
//! it, and writes it back. The store indexes a frame only after its fsync, and
//! its index keeps whichever write of a key lands last, so two writers of one
//! execution on two threads could each write back their own copy and lose the
//! other's update: a turn's commit putting `running` back over a cancel, say.
//! So every such transition holds its execution's lock from the read until its
//! frame is indexed: the append, its fsync, and the index update. Actions
//! belong to their execution, and its lock covers them.
//!
//! The locks are the set of executions locked right now, under one mutex, with
//! a condvar for the waiters. An id is in the set exactly while a transition
//! holds it, so there is nothing to prune when an execution ends, and nothing
//! a late writer of an ended execution could find stale. Writers of different
//! executions never wait for each other: they share only the moment it takes
//! to add or remove an id. Readers that write nothing take no lock, and their
//! view can be one frame stale.
//!
//! A transition that touches two executions takes both in id order
//! (`lock_two`), so two such transitions naming the same pair in either order
//! cannot deadlock. A lock is never taken twice on one thread: a transition
//! that calls another which locks the same execution would wait on itself, so
//! that panics instead.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

#[derive(Default)]
pub(crate) struct ExecLocks {
    state: Mutex<Held>,
    freed: Condvar,
}

#[derive(Default)]
struct Held {
    /// Each locked execution, and the thread that holds it.
    by: HashMap<String, ThreadId>,
    /// Lockers waiting for an execution another thread holds.
    waiting: usize,
}

/// Executions locked by one transition; released when dropped, a panic
/// included.
#[must_use = "the lock is released when this is dropped"]
pub(crate) struct ExecLock<'a> {
    locks: &'a ExecLocks,
    ids: Vec<String>,
}

impl ExecLocks {
    /// The set is only ever held for an insert or a removal, so a panic
    /// elsewhere never leaves it half-changed.
    fn state(&self) -> MutexGuard<'_, Held> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lock one execution, waiting while another thread holds it.
    pub(crate) fn lock(&self, id: &str) -> ExecLock<'_> {
        self.lock_all(&[id])
    }

    /// Lock two executions, in id order: the ordered two-lock helper. DD7's
    /// carved budgets need it (a child's spend counts against its parent's);
    /// nothing calls it yet.
    #[allow(dead_code)]
    pub(crate) fn lock_two(&self, a: &str, b: &str) -> ExecLock<'_> {
        self.lock_all(&[a, b])
    }

    /// Lock every execution named, one at a time in id order, so that two
    /// callers naming the same ones in any order cannot deadlock.
    pub(crate) fn lock_all(&self, ids: &[&str]) -> ExecLock<'_> {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let me = std::thread::current().id();
        // Built first, so a panic below releases what it already took.
        let mut lock = ExecLock {
            locks: self,
            ids: Vec::with_capacity(ids.len()),
        };
        let mut held = self.state();
        for id in ids {
            loop {
                match held.by.get(id) {
                    None => break,
                    Some(t) if *t == me => {
                        drop(held);
                        panic!(
                            "kernel: execution {id} locked twice on one thread (a transition \
                             called another that locks it)"
                        );
                    }
                    Some(_) => {
                        held.waiting += 1;
                        held = self
                            .freed
                            .wait(held)
                            .unwrap_or_else(PoisonError::into_inner);
                        held.waiting -= 1;
                    }
                }
            }
            held.by.insert(id.to_string(), me);
            lock.ids.push(id.to_string());
        }
        lock
    }

    /// How many lockers are waiting now (a test's way to see a writer blocked).
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.state().waiting
    }

    /// How many executions are locked now.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.state().by.len()
    }
}

impl Drop for ExecLock<'_> {
    fn drop(&mut self) {
        if self.ids.is_empty() {
            return;
        }
        let mut held = self.locks.state();
        for id in &self.ids {
            held.by.remove(id);
        }
        drop(held);
        self.locks.freed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    /// Two transitions that take the same two executions, one naming them
    /// (a, b) and the other (b, a), 20,000 times each beside a third that
    /// takes one of them alone: none deadlocks, and no two are ever inside
    /// at once.
    #[test]
    fn two_locks_taken_in_opposite_orders_never_deadlock() {
        let locks = Arc::new(ExecLocks::default());
        let inside = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let takers: [&[&'static str]; 3] = [&["exe_a", "exe_b"], &["exe_b", "exe_a"], &["exe_a"]];
        for ids in takers {
            let (locks, inside, tx) = (locks.clone(), inside.clone(), tx.clone());
            std::thread::spawn(move || {
                for _ in 0..20_000 {
                    let _held = match ids {
                        [a, b] => locks.lock_two(a, b),
                        [a] => locks.lock(a),
                        _ => unreachable!(),
                    };
                    assert_eq!(inside.fetch_add(1, SeqCst), 0, "two takers inside at once");
                    inside.fetch_sub(1, SeqCst);
                }
                tx.send(()).unwrap();
            });
        }
        for _ in takers {
            rx.recv_timeout(Duration::from_secs(20))
                .expect("deadlock: the same two executions locked in opposite orders");
        }
        assert_eq!(locks.held(), 0);
    }

    /// A transition that calls another which locks the same execution would
    /// wait on itself forever; it panics instead, and what it had taken is
    /// released.
    #[test]
    fn a_lock_taken_twice_on_one_thread_panics_and_releases_what_it_took() {
        let locks = ExecLocks::default();
        let b = locks.lock("exe_b");
        let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drop(locks.lock_two("exe_a", "exe_b"))
        }));
        let msg = again.expect_err("a second lock of exe_b on this thread");
        let msg = msg.downcast_ref::<String>().unwrap();
        assert!(msg.contains("exe_b locked twice on one thread"), "{msg}");
        assert_eq!(locks.held(), 1, "exe_a was released, exe_b is still held");
        drop(b);
        assert_eq!(locks.held(), 0);
        drop(locks.lock("exe_a"));
    }
}
