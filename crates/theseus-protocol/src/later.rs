//! What a turn left for later, on a daemon `theseus --spawn ask` started for
//! one run (theseus-mqxk): the jobs still running, a late result whose turn
//! is queued, and the wakes still pending. `ask` follows them while its
//! `--follow-for` lasts and names what is left when it ends, since the
//! daemon ends with it. A socket daemon's results never carry it: its wakes
//! and late results come back on their own.

use serde::{Deserialize, Serialize};

/// A turn's leftovers, as its daemon's kernel held them as the turn ended.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Later {
    /// Jobs its turns left running (answered `background`), not yet settled.
    pub jobs: u32,
    /// A result came back that no turn has read yet: a turn for it is queued.
    pub queued: bool,
    /// Its pending wakes, the soonest first.
    #[serde(default)]
    pub wakes: Vec<LaterWake>,
}

/// One pending wake.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LaterWake {
    pub wake_id: String,
    /// When it is due, in Unix ms.
    pub due_at_ms: u64,
    pub note: String,
}

impl Later {
    /// Nothing is left: no job, no queued turn, no wake.
    pub fn is_empty(&self) -> bool {
        self.jobs == 0 && !self.queued && self.wakes.is_empty()
    }

    /// Whether a turn may still come before `deadline_ms`: a job runs, a
    /// turn is queued, or a wake is due by then.
    pub fn comes_by(&self, deadline_ms: u64) -> bool {
        self.jobs > 0 || self.queued || self.wakes.iter().any(|w| w.due_at_ms <= deadline_ms)
    }
}
