//! The input turns that have not yet taken their execution's hold, by
//! session: a `/stop` that lands while one waits for its secrets, for a slot
//! or for the execution's previous turn reaches it, as it does a turn that
//! holds the execution, since the turn stops itself on the stop's notice once
//! it is admitted (theseus-hmwv). The stop's answer, the session's note of it
//! and the kernel's row of it count such a turn as running. A turn is listed
//! from the start of `TurnRunner::run`, so nothing before its admission is
//! out of the list.
//!
//! A turn that leaves the list for a reason that is not the stop (its
//! secrets refused, its execution not runnable) was counted by a stop that
//! asked while it was listed, and the stop's answer had gone by then. The
//! entry remembers that it was asked (`Waiting::was_counted`), so the turn's
//! runner can say what in fact ended it (`StopNotTaken`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The turns that wait now: each one's session, the arrival of its input,
/// and whether a stop counted it.
#[derive(Default)]
pub struct AdmitWaiting {
    turns: Mutex<Vec<(String, Instant, Arc<AtomicBool>)>>,
}

/// A turn's place in the list, until it holds its execution.
pub(super) struct Waiting<'a> {
    from: &'a AdmitWaiting,
    counted: Arc<AtomicBool>,
    held: AtomicBool,
}

impl AdmitWaiting {
    /// `session_id`'s turn for the input that arrived at `arrived` begins to
    /// wait; it is listed until the guard drops, or `held` is said.
    pub(super) fn enter(&self, session_id: &str, arrived: Instant) -> Waiting<'_> {
        let counted = Arc::new(AtomicBool::new(false));
        self.turns
            .lock()
            .unwrap()
            .push((session_id.to_string(), arrived, counted.clone()));
        Waiting {
            from: self,
            counted,
            held: AtomicBool::new(false),
        }
    }

    /// Whether a turn of `session_id` waits whose input arrived before `at`:
    /// the turns a stop that landed at `at` reaches. Each such turn
    /// remembers that a stop counted it.
    pub fn reaches(&self, session_id: &str, at: Instant) -> bool {
        let turns = self.turns.lock().unwrap();
        let mut any = false;
        for (_, _, counted) in turns
            .iter()
            .filter(|(s, arrived, _)| s == session_id && *arrived < at)
        {
            counted.store(true, Ordering::SeqCst);
            any = true;
        }
        any
    }

    fn leave(&self, counted: &Arc<AtomicBool>) {
        let mut turns = self.turns.lock().unwrap();
        if let Some(at) = turns.iter().position(|t| Arc::ptr_eq(&t.2, counted)) {
            turns.swap_remove(at);
        }
    }
}

impl Waiting<'_> {
    /// The turn holds its execution: it is no longer waiting, and the kernel
    /// counts it from here.
    pub(super) fn held(&self) {
        self.held.store(true, Ordering::SeqCst);
        self.from.leave(&self.counted);
    }

    /// Whether the turn was admitted.
    pub(super) fn is_held(&self) -> bool {
        self.held.load(Ordering::SeqCst)
    }

    /// Leaves the list, and says whether a stop counted the turn while it
    /// was listed: decided under the list's lock, so a stop is either counted
    /// here or not counted at all.
    pub(super) fn withdraw(&self) -> bool {
        self.from.leave(&self.counted);
        self.counted.load(Ordering::SeqCst)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.from.leave(&self.counted);
    }
}
