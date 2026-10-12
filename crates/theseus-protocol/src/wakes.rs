//! A pending wake on the wire (DD8, theseus-cff; moved out of lib.rs at
//! theseus-lherl): `WakeInfo`, which `wake.list`, health, `/wakes` and
//! `theseus wakes` show.

use serde::{Deserialize, Serialize};

/// A pending wake (DD8, theseus-cff): a conversation asked, with `wake.at`,
/// for a turn at a time, whose input is its note. It has not run yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeInfo {
    /// `wak_…`.
    pub wake_id: String,
    /// The last six characters of its id, which is how people name it
    /// (`/cancel a1b2c3`).
    pub short: String,
    pub session_id: String,
    pub execution_id: String,
    /// The session's title, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_title: Option<String>,
    /// The task that set it, by its short id (`a1b2c3`), when its session is a task (37b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<String>,
    pub due_at_ms: u64,
    /// The due time on the daemon's clock, as people read it
    /// (`2026-09-30 13:15:00 -07:00`).
    pub due_local: String,
    pub note: String,
    pub set_at_ms: u64,
    /// The place it was set from (`discord:dm:<user>`), where its turn's
    /// reply goes, if anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    /// Its session's execution state now: a wake waits for a busy session.
    pub state: String,
    /// A repeating wake's span (`1d`, `30m`; 37a); none for a one-shot wake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub every: Option<String>,
    /// Which occurrence of its series is due next, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub occurrence: Option<u32>,
    /// When a series' next occurrence is due, as people read it on the
    /// daemon's clock (`21:00 Thu`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<String>,
    /// A daily series' weekdays (`mon`, `fri`), when it is limited to some;
    /// none for any other wake.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<String>,
    /// When a series ends: no occurrence after it (Unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
    /// The note's first line, clipped: what people call the wake
    /// (theseus-lherl).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
}
