//! Who still holds the store when the runtime is gone (theseus-8ub95). Every
//! handle on a store is gone by the process's end, or redb never closes it.
//! The stop's checkpoint is durable by itself (`WalStore::checkpoint_for_close`),
//! so a handle that outlives the runtime loses nothing; this says that one did,
//! and how many, so the thread or task that holds it is found and joined.

use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

use crate::store::WalStore;

/// Whether the store's state has been dropped, which is when its index closes.
#[derive(Default)]
pub(crate) struct Gate {
    done: Mutex<bool>,
    closed: Condvar,
}

/// The last field of the store's state: its drop is the close, signalled to
/// whoever waits (`wait_closed`). Fields drop in order, so the index and the
/// log have closed before this says so.
#[derive(Default)]
pub(crate) struct CloseMark(Arc<Gate>);

impl CloseMark {
    pub(crate) fn gate(&self) -> Arc<Gate> {
        self.0.clone()
    }
}

impl Drop for CloseMark {
    fn drop(&mut self) {
        *self.0.done.lock().unwrap() = true;
        self.0.closed.notify_all();
    }
}

static WATCHED: Mutex<Option<(Weak<WalStore>, Arc<Gate>)>> = Mutex::new(None);

/// Remember the store, by `Weak`: this never keeps it open.
pub fn watch(store: &Arc<WalStore>) {
    *WATCHED.lock().unwrap() = Some((Arc::downgrade(store), store.close_gate()));
}

/// Wait until the watched store has closed, for at most `bound` (theseus-q0re4).
/// A thread that upgrades a `Weak` to the core for a unit of work can be
/// mid-unit when the runtime is dropped, and holds the store until the unit
/// ends: the stop waits for that, as for a task, instead of racing it.
/// Returns at once when none was watched or the store is already closed, and
/// says whether it closed.
pub fn wait_closed(bound: Duration) -> bool {
    let gate = match WATCHED.lock().unwrap().as_ref() {
        Some((_, gate)) => gate.clone(),
        None => return true,
    };
    let done = gate.done.lock().unwrap();
    let (done, _) = gate
        .closed
        .wait_timeout_while(done, bound, |closed| !*closed)
        .unwrap();
    *done
}

/// The handles on the watched store that are still alive (0: it has closed,
/// or none was watched).
pub fn holders() -> usize {
    WATCHED
        .lock()
        .unwrap()
        .as_ref()
        .map_or(0, |(store, _)| Weak::strong_count(store))
}
