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
//! (`lock_all`): a task and its parent (DD7), opening the task and carving its
//! budget, each of its settles, and its end. So two such transitions naming
//! the same pair in either order cannot deadlock. A kernel transaction
//! (`Kernel::frame`, theseus-0owd) takes every lock its transitions need
//! first, in the same order, and they take none of their own. A lock is never taken twice on one thread: a transition
//! that calls another which locks the same execution would wait on itself, so
//! that panics instead.
//!
//! Nor is a lock taken while the thread holds another (theseus-oqxw). A
//! transaction's closure that calls the kernel itself, not its view, on an
//! execution the frame did not name would take that lock while holding the
//! frame's, out of id order, and two such threads would deadlock with nothing
//! to say so. So `lock_all` panics when its thread holds any lock already:
//! every lock a thread needs at once is taken in one call. The check is a
//! count per thread, so it costs every transition one thread-local read.
//!
//! A lock belongs to its OS thread, so it is `!Send` (Review 2's R7): held
//! across an `.await`, in a future a runtime may move between threads, it is
//! a compile error rather than a "locked twice" panic in an unrelated task. A
//! wait for a lock another thread holds (across its fsync) holds no runtime
//! worker (`theseus_store::blocking`, theseus-vni9).

use std::cell::Cell;
use std::collections::HashMap;
use std::marker::PhantomData;
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

thread_local! {
    /// How many `ExecLock`s this thread holds now, of any `ExecLocks`.
    static HELD_HERE: Cell<u32> = const { Cell::new(0) };
}

/// Executions locked by one transition; released when dropped, a panic
/// included.
#[must_use = "the lock is released when this is dropped"]
pub(crate) struct ExecLock<'a> {
    locks: &'a ExecLocks,
    ids: Vec<String>,
    /// `!Send`: the lock is its thread's (R7).
    _thread: PhantomData<*const ()>,
}

// R7, held at build time: an `ExecLock` that became `Send` fails to compile
// here (the gate runs no doctests). Two impls apply to a `Send` type, so the
// trait's parameter is ambiguous for it.
const _: fn() = || {
    trait AmbiguousIfSend<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    #[allow(dead_code)]
    struct Invalid;
    impl<T: ?Sized + Send> AmbiguousIfSend<Invalid> for T {}
    let _ = <ExecLock<'static> as AmbiguousIfSend<_>>::some_item;
};

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

    /// No lock: what a transition holds inside a kernel transaction, which
    /// took every lock it needs first (`Kernel::frame`, theseus-0owd).
    pub(crate) fn none(&self) -> ExecLock<'_> {
        ExecLock {
            locks: self,
            ids: Vec::new(),
            _thread: PhantomData,
        }
    }

    /// Lock every execution named, one at a time in id order, so that two
    /// callers naming the same ones in any order cannot deadlock.
    pub(crate) fn lock_all(&self, ids: &[&str]) -> ExecLock<'_> {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let me = std::thread::current().id();
        let mut held = self.state();
        if !ids.is_empty() && HELD_HERE.get() > 0 {
            let twice = ids.iter().find(|id| held.by.get(**id) == Some(&me));
            drop(held);
            match twice {
                Some(id) => panic!(
                    "kernel: execution {id} locked twice on one thread (a transition called \
                     another that locks it)"
                ),
                None => panic!(
                    "kernel: a lock taken while this thread holds another: name it in the one \
                     Kernel::frame (locking {ids:?})"
                ),
            }
        }
        // Built first, so a panic below releases what it already took.
        let mut lock = ExecLock {
            locks: self,
            ids: Vec::with_capacity(ids.len()),
            _thread: PhantomData,
        };
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
                        held = theseus_store::blocking(|| {
                            self.freed
                                .wait(held)
                                .unwrap_or_else(PoisonError::into_inner)
                        });
                        held.waiting -= 1;
                    }
                }
            }
            held.by.insert(id.to_string(), me);
            if lock.ids.is_empty() {
                HELD_HERE.set(HELD_HERE.get() + 1);
            }
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
        HELD_HERE.set(HELD_HERE.get().saturating_sub(1));
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
                        [a, b] => locks.lock_all(&[a, b]),
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
            drop(locks.lock_all(&["exe_a", "exe_b"]))
        }));
        let msg = again.expect_err("a second lock of exe_b on this thread");
        let msg = msg.downcast_ref::<String>().unwrap();
        assert!(msg.contains("exe_b locked twice on one thread"), "{msg}");
        assert_eq!(locks.held(), 1, "exe_a was released, exe_b is still held");
        drop(b);
        assert_eq!(locks.held(), 0);
        drop(locks.lock("exe_a"));
    }

    /// theseus-oqxw: a lock of another execution, taken while this thread
    /// holds one, of these locks or of another kernel's, panics and takes
    /// nothing; once the first is released the thread locks again.
    #[test]
    fn a_lock_taken_while_this_thread_holds_another_panics_and_takes_nothing() {
        let (locks, others) = (ExecLocks::default(), ExecLocks::default());
        for held_by in [&locks, &others] {
            let a = held_by.lock("exe_a");
            let nested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(locks.lock_all(&["exe_b", "exe_c"]))
            }));
            let msg = nested.expect_err("a second lock while exe_a is held");
            let msg = msg.downcast_ref::<String>().unwrap();
            assert!(
                msg.contains("a lock taken while this thread holds another"),
                "{msg}"
            );
            assert_eq!(locks.held() + others.held(), 1, "only exe_a is held");
            drop(a);
            drop(locks.lock_all(&["exe_b", "exe_c"]));
        }
        // Another thread's lock is no reason: it waits, as before.
        let a = locks.lock("exe_a");
        std::thread::scope(|s| {
            s.spawn(|| drop(locks.lock("exe_b"))).join().unwrap();
        });
        drop(a);
        assert_eq!(locks.held(), 0);
    }
}
