//! Routines on the wire (theseus-pvm5c; the horizon report's §5.1, §5.4): a
//! recurring piece of work that is its own execution, whose every run is a
//! fresh task with its own budget, and whose month has a limit.
//!
//! - `RoutineSpec` is what a routine adds to its session, kept as the META
//!   record `routine:<session id>` (no store format of its own).
//! - `RoutineInfo` is what `routine.list` and the verbs answer; `RunRow` is one
//!   of its runs, as `routine.runs` lists them.

use serde::{Deserialize, Serialize};

/// The META key of a routine's record: `routine:<session id>`.
pub const KEY_PREFIX: &str = "routine:";

/// The META key of a run's mark (`routine.run:<task session id>`): which
/// routine opened it and how it speaks.
pub const RUN_PREFIX: &str = "routine.run:";

/// The marker a brief may end its report with, for a routine that speaks only
/// when there is something to say.
pub const NOTHING_NEW: &str = "NOTHING NEW";

/// What a routine does after the daemon was down across occurrences.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CatchUp {
    /// Spans of an hour or more run once; shorter ones wait for the next.
    #[default]
    Auto,
    /// Run once, saying how many were missed.
    One,
    /// Run nothing: wait for the next occurrence.
    None,
}

impl CatchUp {
    pub fn parse(s: &str) -> Option<CatchUp> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(CatchUp::Auto),
            "one" => Some(CatchUp::One),
            "none" => Some(CatchUp::None),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CatchUp::Auto => "auto",
            CatchUp::One => "one",
            CatchUp::None => "none",
        }
    }
}

/// Whether a run's report is always posted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Speak {
    /// Every run posts its report.
    #[default]
    Always,
    /// A run whose brief ends in `NOTHING NEW` only logs.
    WhenNotable,
}

impl Speak {
    pub fn parse(s: &str) -> Option<Speak> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "always" | "" => Some(Speak::Always),
            "when_notable" | "notable" => Some(Speak::WhenNotable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Speak::Always => "always",
            Speak::WhenNotable => "when_notable",
        }
    }
}

/// A routine's record: what the owner (or the model, behind a card) asked for,
/// and what the runs have done to it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineSpec {
    /// The routine's session, which is also its id.
    pub session_id: String,
    pub execution_id: String,
    /// The wake that holds its schedule.
    pub wake_id: String,
    pub title: String,
    /// The schedule as it was written (`weekdays 08:00`).
    pub schedule: String,
    /// What each run does.
    pub brief: String,
    /// The most one run may spend, and the routine's month, in micro-dollars.
    pub run_micros: u64,
    pub month_micros: u64,
    #[serde(default)]
    pub catch_up: CatchUp,
    #[serde(default)]
    pub speak: Speak,
    /// Where reports go: `dm` (the owner's DM, the default), `none`, or a
    /// place (`discord:channel:<id>`).
    pub deliver_to: String,
    /// The schedule's shortest gap, which `catch_up: auto` reads.
    pub period_ms: u64,
    pub created_ms: u64,
    /// Who made it: `owner` or `model:<session id>`.
    pub created_by: String,
    /// The conversation that made it, when a model did: its commitments block
    /// lists the routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub created_in: Option<String>,
    /// The local month the spend counts (`2026-10`).
    pub month: String,
    /// The month's spent budget has been said once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub month_told: bool,
    /// Runs opened so far, and the last one's execution.
    #[serde(default)]
    pub runs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_task: Option<String>,
    /// Deleted at this time: kept as a record, listed no more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub deleted_ms: Option<u64>,
}

/// A run's mark: the META record `routine.run:<task session id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMark {
    pub routine: String,
    pub run: u32,
    pub speak: Speak,
}

/// One run of a routine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RunRow {
    /// The task's session id, which names the run.
    pub task: String,
    pub short: String,
    pub run: u32,
    /// `queued`, `running`, `complete`, `failed`, or `cancelled`.
    pub state: String,
    pub started_ms: u64,
    pub started_local: String,
    /// How long it ran, or has run.
    pub duration_ms: u64,
    pub cost_usd: f64,
    /// The first line of its report, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<String>,
}

/// A routine, as the verbs and the list show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineInfo {
    /// The routine's session id; the last six characters name it (`short`).
    pub id: String,
    pub short: String,
    pub title: String,
    pub schedule: String,
    /// The series as `describe` words it (`weekdays 08:00`).
    pub series: String,
    pub brief: String,
    pub paused: bool,
    /// When the next run is due (Unix ms), and as people read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<String>,
    pub run_usd: f64,
    pub month_usd: f64,
    /// The month's spend so far (zero once the month has turned).
    pub month_spent_usd: f64,
    pub deliver_to: String,
    pub catch_up: String,
    pub speak: String,
    pub runs: u32,
    /// A run is going now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last: Option<RunRow>,
}

/// `routine.create`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineCreateParams {
    pub title: String,
    /// `weekdays 08:00`, `every 30m`, `monthly 09:00`, or cron.
    pub schedule: String,
    pub brief: String,
    /// The most one run may spend, in dollars (default `[routines] default_run_usd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget_usd: Option<f64>,
    /// The routine's month, in dollars (default `[routines] default_month_usd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub month_usd: Option<f64>,
    /// `dm` (default), `none`, or a place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub deliver_to: Option<String>,
    /// `auto` (default), `one`, or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub catch_up: Option<String>,
    /// `always` (default) or `when_notable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub speak: Option<String>,
    /// Who asked, as the ledger names them (default: the surface).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `routine.list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineListResult {
    pub routines: Vec<RoutineInfo>,
}

/// A verb on one routine: `routine.resume`, `routine.run_now`, `routine.delete`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineNameParams {
    /// Its id, or the end of it (`a41d04`).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `routine.runs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineRunsParams {
    pub id: String,
    /// The newest this many (default 20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineRunsResult {
    pub routine: RoutineInfo,
    /// Newest first.
    pub runs: Vec<RunRow>,
}

/// `routine.pause`: hold the series, or move it past `until`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutinePauseParams {
    pub id: String,
    /// Not held: the next run is the first at or after this time (`mon`,
    /// `tomorrow 9:00`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `routine.update`: change what is given.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineUpdateParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub schedule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub brief: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub month_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub deliver_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub catch_up: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub speak: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What a verb that changes one routine answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineEditResult {
    pub routine: RoutineInfo,
    /// `run_now`: the run's task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<String>,
}
