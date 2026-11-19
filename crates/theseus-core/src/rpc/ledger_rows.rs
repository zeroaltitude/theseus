//! The rows the index tender's supervisor and the language servers write
//! (`crate::fact::index`, `crate::fact::lsp`): the ledger, off the runtime's
//! workers (a row is an fsync), and in the order they were said.
//!
//! Each row was its own `spawn_blocking`, so two rows said one after the
//! other ran on any two free threads of the pool and landed in either order:
//! the ledger read `lsp.ready` before the `lsp.started` of the same server,
//! 2 of 400 runs of a test beside 64 busy loops (issue 20, the third pass),
//! and a person reading `theseus ledger` would meet a tender "ready" before
//! it "started". One queue and at most one writer at a time: a row says
//! itself into the queue, and whichever task drains it writes in the order
//! of the queue. The queue holds the core by `Weak`, so the writer never keeps
//! the store open past a stop.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

use crate::ledger::LedgerRow;
use crate::Core;

/// The rows not yet written, and whether a writer is draining them.
#[derive(Default)]
pub(super) struct Queue {
    pub(super) rows: VecDeque<LedgerRow>,
    pub(super) draining: bool,
}

/// The sink the tender's and the language servers' hooks write through.
pub(super) fn ordered(core: &Arc<Core>) -> crate::tender::Ledger {
    let core = Arc::downgrade(core);
    let queue: Arc<Mutex<Queue>> = Arc::default();
    Arc::new(move |row: LedgerRow| {
        let start = {
            let mut q = queue.lock().unwrap();
            q.rows.push_back(row);
            !std::mem::replace(&mut q.draining, true)
        };
        if !start {
            return;
        }
        let (core, queue) = (core.clone(), queue.clone());
        let write = move || drain(&core, &queue);
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    })
}

/// Write the queue to its end, oldest first; the writer ends with it.
fn drain(core: &Weak<Core>, queue: &Mutex<Queue>) {
    drain_with(queue, |row| {
        let Some(core) = core.upgrade() else { return };
        if let Err(e) = core.store.append_ledger(&row) {
            tracing::warn!(error = %e, "ledger append failed");
        }
    });
}

/// `drain`, with the write of each row as `write`. A write that panics ends
/// this writer, not the sink: the next row said starts another, and the rows
/// behind the one that failed are still queued for it.
pub(super) fn drain_with(queue: &Mutex<Queue>, mut write: impl FnMut(LedgerRow)) {
    struct Ended<'a>(&'a Mutex<Queue>);
    impl Drop for Ended<'_> {
        fn drop(&mut self) {
            if std::thread::panicking() {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .draining = false;
            }
        }
    }
    let _ended = Ended(queue);
    loop {
        let row = {
            let mut q = queue.lock().unwrap();
            match q.rows.pop_front() {
                Some(row) => row,
                None => {
                    q.draining = false;
                    return;
                }
            }
        };
        write(row);
    }
}
