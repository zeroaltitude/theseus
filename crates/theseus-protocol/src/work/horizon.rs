//! The horizon: one view of the future (theseus-nuhsz; horizon HZ8). A
//! reminder, a routine, a watch, a deferred task and a project are each a
//! `WorkView` on the work board, and `horizon_of` says which of seven
//! horizons a view is on: Now, Today, This week, Later, Recurring, Waiting on
//! the world, and Projects. Types and pure functions only, like `work.rs`:
//! this crate reads no clock and no zone, so the client passes its own day
//! (`LocalDay`) and the time.
//!
//! - The seam (`WorkView`'s `purpose`, `next_at_ms`, `series`, `paused`,
//!   `routine`, `watch`, `project`) is additive: an older peer decodes the
//!   bytes and an older daemon's views read with every field at its default.
//! - `work.agenda` (`AgendaParams`, `AgendaResult`): every occurrence in a
//!   window, the daemon expanding series by the kernel's own arithmetic.
//! - The answers a new kind offers (`routine_failed_answers`,
//!   `watch_met_answers`, `milestone_answers`), each a direct call like a
//!   reminder's.

use serde::{Deserialize, Serialize};

use crate::wakes::WakeAnswer;
use crate::work::{WorkKind, WorkState, WorkView};
use crate::Level;

/// A repeating series, as a view shows it. Exactly one of `every` and `cron`
/// is set: the span (`1d`, `30m`) or the cron expression.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Series {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub every: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cron: Option<String>,
    /// A daily series' weekdays (`mon`, `fri`); empty is every day.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<String>,
    /// No occurrence after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
    /// Which occurrence is due next, from 1.
    pub occurrence: u32,
    /// How many occurrences the last firing passed over (the daemon was
    /// down, or the conversation busy).
    #[serde(default)]
    pub missed_last: u32,
}

/// How a routine's last run ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineLast {
    /// `ok`, `failed`, `blocked`, `cancelled`, or `running`.
    pub outcome: String,
    pub at_ms: u64,
    pub duration_ms: u64,
    pub cost_usd: f64,
    /// The first line of its report, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub line: Option<String>,
}

/// A routine's runs over the last 30 days.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RunCounts {
    pub ok: u32,
    pub failed: u32,
    pub skipped: u32,
}

/// A routine's month: what it spent and what it may.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Month {
    pub spent_usd: f64,
    pub limit_usd: f64,
}

/// What a routine adds to its view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutineView {
    /// The schedule as it was written (`weekdays 08:00`).
    pub schedule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last: Option<RoutineLast>,
    pub runs_30d: RunCounts,
    pub month: Month,
    /// `dm`, `none`, or a place.
    pub deliver_to: String,
}

/// A watch's last value and when it was seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchLast {
    pub value: String,
    pub at_ms: u64,
}

/// What a watch adds to its view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchView {
    /// What it looks at, as people read it: `gh pr view 123`, a URL, a path.
    pub check: String,
    /// The condition as typed (`equals MERGED`).
    pub until: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last: Option<WatchLast>,
    pub checks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub expires_at_ms: Option<u64>,
    /// Why its last checks failed, once three in a row had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub failing: Option<String>,
}

/// Done of a total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Tally {
    pub done: u32,
    pub total: u32,
}

/// The next step or milestone of a project, and when it is due.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NextDue {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub due_at_ms: Option<u64>,
}

/// What a project adds to its view. Declared with the seam; the `projects`
/// row fills it from the task records' `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProjectView {
    pub goal: String,
    pub steps: Tally,
    pub milestones: Tally,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<NextDue>,
    /// Milestones past their date.
    #[serde(default)]
    pub overdue: u32,
    /// When its weekly review is due or was held, as people read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub review: Option<String>,
}

/// Where a piece of work sits on the future's axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Horizon {
    /// Running, queued, overdue, or needing you.
    Now,
    /// Due before tomorrow's midnight on the client's clock.
    Today,
    /// Due in the next seven local days after today.
    Week,
    /// Due after that, or with no date.
    Later,
    /// A series: a routine, a repeating reminder.
    Recurring,
    /// A watch: waiting on the world.
    World,
    /// A project.
    Projects,
}

impl Horizon {
    pub const ALL: [Horizon; 7] = [
        Horizon::Now,
        Horizon::Today,
        Horizon::Week,
        Horizon::Later,
        Horizon::Recurring,
        Horizon::World,
        Horizon::Projects,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Horizon::Now => "now",
            Horizon::Today => "today",
            Horizon::Week => "week",
            Horizon::Later => "later",
            Horizon::Recurring => "recurring",
            Horizon::World => "world",
            Horizon::Projects => "projects",
        }
    }

    /// The heading a surface shows.
    pub fn title(self) -> &'static str {
        match self {
            Horizon::Now => "Now",
            Horizon::Today => "Today",
            Horizon::Week => "This week",
            Horizon::Later => "Later",
            Horizon::Recurring => "Recurring",
            Horizon::World => "Waiting on the world",
            Horizon::Projects => "Projects",
        }
    }
}

/// The client's day, in its own zone: the crate reads no zone, so the client
/// says where today ends and the week with it. Both are local midnights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LocalDay {
    /// The start of the next local day: what is due before it is Today.
    pub tomorrow_ms: u64,
    /// The start of the eighth local day (today and the seven after it): what
    /// is due before it, and not Today, is This week.
    pub week_end_ms: u64,
}

/// A time's horizon among the dated ones: due already is Now, before
/// tomorrow's midnight Today, before the week's end This week, else Later.
pub fn bucket_at(at_ms: u64, now_ms: u64, day: &LocalDay) -> Horizon {
    if at_ms <= now_ms {
        Horizon::Now
    } else if at_ms < day.tomorrow_ms {
        Horizon::Today
    } else if at_ms < day.week_end_ms {
        Horizon::Week
    } else {
        Horizon::Later
    }
}

/// Which horizon a view is on. First match wins:
///
/// | # | When | Horizon |
/// |---|---|---|
/// | 1 | a project | Projects |
/// | 2 | ended and not needing you | Later (a surface does not list it) |
/// | 3 | needs you (a failed run, a blocked task) | Now |
/// | 4 | a watch | Waiting on the world |
/// | 5 | running or queued | Now |
/// | 6 | a routine, or a wake or task with a series | Recurring |
/// | 7 | a dated view | by its time (`bucket_at`) |
/// | 8 | a wake with no time | Later |
/// | 9 | anything else (a conversation at rest, a task on its calls) | Now |
///
/// Pure: `now_ms` and `day` are the client's.
pub fn horizon_of(v: &WorkView, now_ms: u64, day: &LocalDay) -> Horizon {
    if v.kind == WorkKind::Project {
        return Horizon::Projects;
    }
    let needs_you = v.attention.level == Level::NeedsYou;
    if v.state.ended() && !needs_you {
        return Horizon::Later;
    }
    if needs_you {
        return Horizon::Now;
    }
    if v.kind == WorkKind::Watch || v.purpose.as_deref() == Some("watch") {
        return Horizon::World;
    }
    if matches!(v.state, WorkState::Running | WorkState::Queued) {
        return Horizon::Now;
    }
    if v.kind == WorkKind::Routine || v.series.is_some() {
        return Horizon::Recurring;
    }
    match v.next_at_ms {
        Some(at) => bucket_at(at, now_ms, day),
        None if v.kind == WorkKind::Wake => Horizon::Later,
        None => Horizon::Now,
    }
}

/// A mail watch's line on the board: how long it has waited for a reply and,
/// when a follow-up is set, the day it follows up on (`follow_up_day`, as the
/// caller words a weekday: `Fri`): `waiting: reply on 'Dana?' (2 d, follow-up
/// Fri)`. Pure, so every surface shows the same words.
pub fn reply_wait(title: &str, waited_ms: u64, follow_up_day: Option<&str>) -> String {
    let age = match waited_ms / 60_000 {
        m if m >= 1440 => format!("{} d", m / 1440),
        m if m >= 60 => format!("{} h", m / 60),
        m => format!("{m} min"),
    };
    let then = follow_up_day.map_or_else(String::new, |d| format!(", follow-up {d}"));
    format!("waiting: reply on '{}' ({age}{then})", title.trim())
}

/// `work.agenda`: every occurrence in a window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AgendaParams {
    /// The window, `[from_ms, to_ms)`.
    pub from_ms: u64,
    pub to_ms: u64,
    /// Only these conversations (session ids); a surface that reads no
    /// private text must name them, as `work.watch` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub roots: Option<Vec<String>>,
}

/// What an agenda line is: the occurrence's verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AgendaWhat {
    /// A reminder or a check-in falls due.
    Due,
    /// A routine's run.
    Run,
    /// A deferred task starts.
    Starts,
    /// A watch gives up.
    Expires,
    /// A milestone's date.
    Milestone,
}

impl AgendaWhat {
    pub fn as_str(self) -> &'static str {
        match self {
            AgendaWhat::Due => "due",
            AgendaWhat::Run => "run",
            AgendaWhat::Starts => "starts",
            AgendaWhat::Expires => "expires",
            AgendaWhat::Milestone => "milestone",
        }
    }
}

/// One occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AgendaLine {
    pub at_ms: u64,
    /// The work it belongs to (a wake's, a task's, a watch's id).
    pub id: String,
    pub kind: WorkKind,
    pub what: AgendaWhat,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub purpose: Option<String>,
    /// Which occurrence of its series this is, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub occurrence: Option<u32>,
}

/// `work.agenda`'s answer: the window's occurrences, soonest first, at most
/// `AGENDA_MAX`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AgendaResult {
    pub from_ms: u64,
    pub to_ms: u64,
    pub lines: Vec<AgendaLine>,
    /// More fell in the window than the lines hold.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub more: bool,
}

/// The most lines `work.agenda` answers.
pub const AGENDA_MAX: usize = 200;

fn answer(id: &str, label: &str, method: &str) -> WakeAnswer {
    WakeAnswer {
        id: id.into(),
        label: label.into(),
        method: method.into(),
        when: None,
    }
}

/// A failed or blocked routine's answers: run it again, hold it, or see its
/// runs.
pub fn routine_failed_answers() -> Vec<WakeAnswer> {
    vec![
        answer("run_now", "Run now", crate::method::ROUTINE_RUN_NOW),
        answer("pause", "Pause", crate::method::ROUTINE_PAUSE),
        answer("details", "Details", crate::method::ROUTINE_RUNS),
    ]
}

/// A met watch's answers: it is done, or watch for it again. "Keep watching"
/// has no verb yet (`watch.keep`, a join note): a surface offers it only when
/// its method is one the daemon answers.
pub fn watch_met_answers() -> Vec<WakeAnswer> {
    vec![
        answer("done", "Done", crate::method::WAKE_DONE),
        answer("keep", "Keep watching", "watch.keep"),
    ]
}

/// An overdue milestone's answers: open the project, or move the date. The
/// move has no verb until the `projects` row brings `project.move`.
pub fn milestone_answers() -> Vec<WakeAnswer> {
    vec![
        answer("open", "Open", crate::method::PROJECT_SHOW),
        answer("move", "Move the date", "project.move"),
    ]
}

/// One digest line for a ledger row about the future, in the digest's words
/// (theseus-nuhsz): a reminder or a routine's firing (late, or after the
/// daemon was down, and how many occurrences it passed over), a run skipped
/// and why, a watch met, given up or failing. The rows carry no title, so the
/// reader passes the work's (`title`, from its view) and how to write a time
/// of day. `None` for a row the digest does not tell. The reader is
/// `work-digest`; a routine run's end is the task's own notice (rule 8).
pub fn digest_line(
    kind: &str,
    data: &serde_json::Value,
    title: &str,
    hm: &dyn Fn(u64) -> String,
) -> Option<String> {
    let n = |k: &str| data[k].as_u64().unwrap_or(0);
    let due = hm(n("due_at_ms"));
    match kind {
        "wake.fired" => {
            let routine = data["purpose"].as_str() == Some("routine");
            let mut line = if routine {
                format!("🔁 {title} ran at {due}")
            } else {
                format!("⏰ {title} fired at {due}")
            };
            if n("late_ms") > 5_000 {
                let down = if data["while_down"].as_bool().unwrap_or(false) {
                    ", the daemon was down"
                } else {
                    ""
                };
                line.push_str(&format!(" ({} late{down})", span(n("late_ms"))));
            }
            if n("missed") > 0 {
                line.push_str(&format!(
                    " · {} missed while the daemon was down",
                    n("missed")
                ));
            }
            Some(line)
        }
        "wake.skipped" => {
            let why = match data["why"].as_str().unwrap_or("") {
                "overlap" => "a run was still going",
                "budget" => "the month's budget was spent",
                "catch_up" => "the daemon was down and it catches up on none",
                "owner" => "the owner declined it",
                other => other,
            };
            Some(format!("🔁 {title} skipped at {due}: {why}"))
        }
        "watch.met" => Some(format!(
            "👁 {title} met: {}",
            data["value"].as_str().unwrap_or("")
        )),
        "watch.expired" => Some(format!("👁 {title} gave up after {} checks", n("checks"))),
        "watch.failing" => Some(format!(
            "👁 {title} is failing: {}",
            data["error"].as_str().unwrap_or("")
        )),
        _ => None,
    }
}

/// A span as people say it: `40 s`, `14 min`, `2 h 5 min`.
fn span(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ if s % 3600 / 60 == 0 => format!("{} h", s / 3600),
        _ => format!("{} h {} min", s / 3600, s % 3600 / 60),
    }
}

#[cfg(test)]
#[path = "tests_horizon.rs"]
mod tests;
