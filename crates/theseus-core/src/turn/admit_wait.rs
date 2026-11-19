//! The input turns waiting in `admit()`, by execution: a `/stop` that lands
//! while one waits for its slot or for the execution's previous turn reaches
//! it, as it does a turn that holds the execution, since the turn stops
//! itself on the stop's notice once it is admitted (theseus-hmwv). The stop's
//! answer, the session's note of it and the kernel's row of it count such a
//! turn as running.

use std::sync::Mutex;
use std::time::Instant;

/// The turns in `admit()` now: each one's execution and the arrival of its
/// input.
#[derive(Default)]
pub struct AdmitWaiting {
    turns: Mutex<Vec<(String, Instant)>>,
}

/// A turn's place in the list, until it leaves `admit()`.
pub(super) struct Waiting<'a> {
    from: &'a AdmitWaiting,
    entry: (String, Instant),
}

impl AdmitWaiting {
    /// `execution_id`'s turn for the input that arrived at `arrived` begins
    /// to wait; it is listed until the guard drops.
    pub(super) fn enter(&self, execution_id: &str, arrived: Instant) -> Waiting<'_> {
        let entry = (execution_id.to_string(), arrived);
        self.turns.lock().unwrap().push(entry.clone());
        Waiting { from: self, entry }
    }

    /// Whether a turn of `execution_id` waits whose input arrived before
    /// `at`: the turns a stop that landed at `at` reaches.
    pub fn reaches(&self, execution_id: &str, at: Instant) -> bool {
        self.turns
            .lock()
            .unwrap()
            .iter()
            .any(|(e, arrived)| e == execution_id && *arrived < at)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let mut turns = self.from.turns.lock().unwrap();
        if let Some(at) = turns.iter().position(|t| *t == self.entry) {
            turns.swap_remove(at);
        }
    }
}
