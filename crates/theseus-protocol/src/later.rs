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
    /// When the run ends, in Unix ms: its bound after the ask's turn ended.
    #[serde(default)]
    pub ends_at_ms: u64,
}

/// One pending wake.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LaterWake {
    pub wake_id: String,
    /// When it is due, in Unix ms.
    pub due_at_ms: u64,
    pub note: String,
    /// It fires before the run ends, as `wake.at` told the model when it was
    /// set; one that does not is left behind.
    #[serde(default)]
    pub fires: bool,
}

impl Later {
    /// Nothing is left: no job, no queued turn, no wake.
    pub fn is_empty(&self) -> bool {
        self.jobs == 0 && !self.queued && self.wakes.is_empty()
    }

    /// Whether a turn may still come before the run ends: a job runs, a
    /// turn is queued, or a wake fires.
    pub fn comes(&self) -> bool {
        self.jobs > 0 || self.queued || self.wakes.iter().any(|w| w.fires)
    }
}
