//! A pending wake on the wire (DD8, theseus-cff; moved out of lib.rs at
//! theseus-lherl): `WakeInfo`, which `wake.list`, health, `/wakes` and
//! `theseus wake` show.

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
    /// Why it exists, when not the model's own check-in: `remind`, `routine`,
    /// `watch` (theseus-8ux6x).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub purpose: Option<String>,
    /// What firing it does, when not a turn: `notice` posts the note and
    /// takes no turn and no model call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub run: Option<String>,
    /// Held by the owner: the due scan passes it over until it is resumed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

/// `wake.create`: a reminder on the owner's word, with no model turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeCreateParams {
    /// When: `in 10m`, `10m`, `tomorrow 9:00`, `fri 9:00`, `17:30`, an RFC 3339
    /// time, or `2026-10-14 09:00`, in the daemon's zone.
    pub when: String,
    /// What it says: the whole message.
    pub note: String,
    /// Repeat it at this span (`30m`, `1d`, `1w`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub every: Option<String>,
    /// With `every` of `1d`: the weekdays it runs on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<String>,
    /// With `every`: no occurrence after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until: Option<String>,
    /// The conversation it lives in; none is the owner's Home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Who asked, as the ledger names them (default: the surface).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

/// What a wake verb answers: the wake as it is after it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeEditResult {
    pub wake: WakeInfo,
    /// It lives in the owner's Home conversation (`wake.create` with no
    /// session).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home: bool,
}

/// `wake.snooze`: a wake's next time moved, or a fired reminder set again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeSnoozeParams {
    /// Its id, or the end of it.
    pub wake: String,
    /// When, as `wake.create`'s `when` says it; none is 10 minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub when: Option<String>,
    /// The wake that just fired (a notice's button): set it again from its
    /// record even if its series has a next occurrence pending.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

/// `wake.pause`: hold a wake, or move it past `until`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakePauseParams {
    pub wake: String,
    /// Not held: a series' next occurrence at or after this time, a one-shot's
    /// time moved to it if later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

/// `wake.resume` and `wake.done`: a wake by name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeNameParams {
    pub wake: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

/// `wake.done`'s answer: the reminder was seen and is settled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeDoneResult {
    pub wake_id: String,
    pub short: String,
}

/// One of a reminder's answers: a button, a menu line, a key. Each is a
/// direct call (a later `work.answer` will route them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeAnswer {
    /// `done`, `snooze_10m`, `snooze_1h`, `tomorrow_9`.
    pub id: String,
    /// `Done`, `Snooze 10 min`, `Snooze 1 h`, `Tomorrow 9:00`.
    pub label: String,
    /// The method it calls: `wake.done` or `wake.snooze`.
    pub method: String,
    /// What it passes as `wake.snooze`'s `when`; none for Done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub when: Option<String>,
}

/// The four answers every reminder carries, in order.
pub fn reminder_answers() -> Vec<WakeAnswer> {
    let a = |id: &str, label: &str, method: &str, when: Option<&str>| WakeAnswer {
        id: id.into(),
        label: label.into(),
        method: method.into(),
        when: when.map(str::to_string),
    };
    vec![
        a("done", "Done", crate::method::WAKE_DONE, None),
        a(
            "snooze_10m",
            "Snooze 10 min",
            crate::method::WAKE_SNOOZE,
            Some("10m"),
        ),
        a(
            "snooze_1h",
            "Snooze 1 h",
            crate::method::WAKE_SNOOZE,
            Some("1h"),
        ),
        a(
            "tomorrow_9",
            "Tomorrow 9:00",
            crate::method::WAKE_SNOOZE,
            Some("tomorrow 9:00"),
        ),
    ]
}

/// `wake.noticed`: a notice wake fired (theseus-8ux6x). The core sends it to
/// every connection once, in the frame's wake: the reminder's Interrupt on
/// every surface. A surface reads `notices::reminder` for its line and
/// urgency, and `answers` for what the owner may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeNoticed {
    pub wake_id: String,
    pub short: String,
    pub session_id: String,
    pub execution_id: String,
    /// `remind`, `routine`, `watch`, or `check`.
    pub purpose: String,
    pub note: String,
    pub due_at_ms: u64,
    /// How long after its time it fired.
    pub late_ms: u64,
    /// It fell due while the daemon was not running.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub while_down: bool,
    /// A series' span (`1d`), when it repeats; and which occurrence this was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub every: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub occurrence: Option<u32>,
    /// Where its note was posted (`discord:dm:<user>`); none when no place
    /// hears it (a placeless Home): only the surfaces do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub posted_to: Option<String>,
    /// It lives in the owner's Home conversation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home: bool,
    pub answers: Vec<WakeAnswer>,
}
