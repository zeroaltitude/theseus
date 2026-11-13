//! The OpenClaw sync's wire types (theseus-pjmj7): `theseus import sync` reads
//! OpenClaw's own session stores (`scripts/openclaw-export.py`) and sends the
//! sessions that changed through `import.episodes`; these two methods are the
//! daemon's half.
//!
//! - **`import.sync.plan`**: the owner's config for the run (`[import.openclaw]`:
//!   the state directory, the owner's sender ids, the agents), the cursor the
//!   last run left, and the first run's default start. A read.
//! - **`import.sync.record`**: the run's end. It writes the new cursor and the
//!   run's counts in one frame (health's `sync`, the `import.synced` row), then,
//!   unless the run failed or was dry, makes the touched tags' people (handles,
//!   no model call) and reads the new messages of the sessions it touched for
//!   people (a model and Jev, under `[people] sweep_usd_per_day`), and writes
//!   the people step's spend.
//! - **`import.sync.request`** (theseus-ievo3): a sync wanted now, rather than
//!   at the next scheduled run. It records the request and answers the plan.
//!   **It starts nothing**: the reading is the CLI's (`openclaw-export.py`),
//!   and what serves a request is the next run of `theseus import sync
//!   --if-requested`, which the on-demand timer takes every few minutes. So
//!   the daemon's half of "on demand" is a record, not a process.
//!
//! All three are the owner's acts, from a private place, refused inside a job.
//! That is the whole of the authorisation, and it is the same rule the plan and
//! the record have always taken (`places::owner_in_private`): the CLI, the web
//! UI, a DM with an owner, or a channel bound private -- so a chat-originated
//! request needs no second mechanism, only a surface that calls this method.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::import::ImportRejected;

/// The minute past `[judge] learning_hour` at which the sync runs: the timer's
/// `OnCalendar` and health's "next run" are both this.
pub const SYNC_MINUTE: u32 = 40;

/// A night past its time by this long, with no run since, is missed.
pub const MISSED_AFTER_MS: u64 = 2 * 3_600_000;

/// A request this old that no run has served is a warning (theseus-ievo3).
/// The on-demand timer's period is minutes, and the import-only timer's is an
/// hour, so an unserved request this old means neither is installed or neither
/// is running: health says so rather than looking well. It is deliberately
/// longer than the hourly import's period, so the hourly run alone -- the
/// fallback when no on-demand unit is installed -- never trips it.
pub const REQUEST_STALE_AFTER_MS: u64 = 75 * 60_000;

/// How many of a run's rejections its stored state names.
pub const SAMPLES: usize = 5;

/// A session an `import.episodes` batch wrote to: created, or appended to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportTouched {
    pub session_id: String,
    /// The tag the session is held under: the one it was first imported in.
    pub tag: String,
}

/// `import.sync.plan`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportSyncPlan {
    /// `[import.openclaw] state_dir` is set.
    pub configured: bool,
    pub state_dir: Option<String>,
    /// The owner's sender ids in OpenClaw's channels, from the owner's config.
    pub owner_ids: Vec<String>,
    /// The agents to read; none is every agent in the state directory.
    pub agents: Vec<String>,
    /// The cursor the last run left (the exporter's own JSON), if any.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub cursor: Option<Value>,
    /// With no cursor, where a first run starts: the newest `as_of` end of
    /// the imported sessions whose source is `openclaw-store`, in unix ms.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub since_ms: Option<u64>,
    /// `[judge] learning_hour`: the timer fires at it, at [`SYNC_MINUTE`].
    pub learning_hour: u8,
    /// Sessions the last run's people step left for this one.
    #[cfg_attr(test, ts(type = "number"))]
    pub people_todo: u64,
    /// When a sync was asked for and no run has served it yet, unix ms; 0 is
    /// none (theseus-ievo3). A run of `--if-requested` runs when this is set
    /// and does nothing when it is not.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub requested_ms: u64,
    /// Who asked, as the act's record labels them; `None` with no request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub requested_by: Option<String>,
}

/// `import.sync.record`'s params: one run's end.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportSyncRecord {
    /// When the run began, unix ms.
    #[cfg_attr(test, ts(type = "number"))]
    pub started_ms: u64,
    /// The exporter's new cursor, written only when the run did not fail.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub cursor: Option<Value>,
    /// Episodes read from the stores, and their messages.
    #[cfg_attr(test, ts(type = "number"))]
    pub episodes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub messages: u64,
    /// What `import.episodes` said, summed.
    #[cfg_attr(test, ts(type = "number"))]
    pub imported: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub appended: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub skipped: u64,
    pub rejected: Vec<ImportRejected>,
    /// The sessions the run created or appended to.
    pub touched: Vec<ImportTouched>,
    /// Why the run failed, when it did: the cursor is then not moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
    /// Skip the people steps (the run's own spend is then nothing).
    #[serde(default)]
    pub no_people: bool,
}

/// What the people steps of a run did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SyncPeople {
    /// Tags whose people were made from the stored records (no model call).
    pub tags: Vec<String>,
    /// People declared, and sessions joined, by those.
    #[cfg_attr(test, ts(type = "number"))]
    pub declared: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub joined: u64,
    /// Sessions whose new messages were read for people, the candidates found,
    /// and those Jev judged.
    #[cfg_attr(test, ts(type = "number"))]
    pub swept: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub candidates: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub judged: u64,
    pub spent_usd: f64,
    /// Sessions left for the next run, and why.
    #[cfg_attr(test, ts(type = "number"))]
    pub left: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}

/// A run's counts, as stored and as health shows them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HealthSync {
    /// When the last run ended, unix ms.
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions_new: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions_appended: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub messages: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub rejected: u64,
    /// The people step's spend, in dollars.
    pub people_usd: f64,
    /// When the next run is due, unix ms: the next [`SYNC_MINUTE`] past
    /// `[judge] learning_hour`.
    #[cfg_attr(test, ts(type = "number"))]
    pub next_ms: u64,
    /// Why the last run failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
    /// A failed or a missed night, in one sentence for every surface's warning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub warn: Option<String>,
    /// A sync asked for that no run has served yet, unix ms; 0 is none
    /// (theseus-ievo3).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub requested_ms: u64,
}

/// `import.sync.record`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImportSyncResult {
    pub sync: HealthSync,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub people: Option<SyncPeople>,
    pub ms: f64,
}
