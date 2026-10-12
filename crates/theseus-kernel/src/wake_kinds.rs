//! What a wake is for, and what firing it does (theseus-8ux6x; the horizon
//! report's §5.0 to §5.1): every future thing is a wake with a purpose, so
//! one record change declares them all and the rows after it (routines,
//! watches) bump nothing.
//!
//! - `Purpose`: why it exists. `Check` is the model's own check-in, which
//!   every wake was before this (the default, so an older record reads as
//!   today's wake); `Remind` is a reminder for the owner; `Routine` and
//!   `Watch` are declared for their rows.
//! - `WakeRun`: what firing does. `Turn` (the default) gives the conversation
//!   a turn whose input is the note; `Notice` posts the note and takes no
//!   turn, no model call and no admission slot; `Task` and `Probe` are
//!   declared and refused (`WakeRun::refusal`) until their rows land.
//!
//! Both leave a record's bytes alone while they hold their default (serde's
//! `skip_serializing_if`), so a wake nothing here touches stays byte for
//! byte what the build before wrote.

use serde::{Deserialize, Serialize};

/// Why a wake exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// The model's own check-in: a turn of its conversation, later.
    #[default]
    Check,
    /// A reminder for the owner: it reaches them where they will hear it.
    Remind,
    /// A recurring job of its own (the routines row).
    Routine,
    /// A check with no model, until a condition holds (the watches row).
    Watch,
}

impl Purpose {
    pub fn is_check(&self) -> bool {
        *self == Purpose::Check
    }

    /// What people call one: `reminder`, `routine`, `watch`, `check-in`.
    pub fn noun(self) -> &'static str {
        match self {
            Purpose::Check => "check-in",
            Purpose::Remind => "reminder",
            Purpose::Routine => "routine",
            Purpose::Watch => "watch",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Check => "check",
            Purpose::Remind => "remind",
            Purpose::Routine => "routine",
            Purpose::Watch => "watch",
        }
    }
}

/// What a routine's fire starts: a fresh task with a brief of its own (the
/// routines row builds it; declared now so the record never changes again).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaskRun {
    /// The task's brief.
    pub brief: String,
    /// Its spend limit in micro-dollars; 0 is the config's.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub limit_micros: u64,
    /// The tools it may use without asking, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

/// What a watch checks, with no model call (the watches row builds it).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WakeProbe {
    /// What it looks at (`pr`, `url`, …).
    pub kind: String,
    /// The thing it looks at, as its kind names it.
    pub subject: String,
    /// What it waits for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub until: String,
}

/// What firing a wake does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeRun {
    /// A turn of its conversation, whose input is the note.
    #[default]
    Turn,
    /// The note is the whole message: posted, with no turn and no model.
    Notice,
    /// A task of its own (declared; refused until the routines row).
    Task(TaskRun),
    /// A probe with no model (declared; refused until the watches row).
    Probe(WakeProbe),
}

impl WakeRun {
    pub fn is_turn(&self) -> bool {
        *self == WakeRun::Turn
    }

    pub fn is_notice(&self) -> bool {
        *self == WakeRun::Notice
    }

    /// Why a tool may not set this kind yet, in words the model or the owner
    /// reads; none for the two that run today.
    pub fn refusal(&self) -> Option<&'static str> {
        match self {
            WakeRun::Turn | WakeRun::Notice => None,
            WakeRun::Task(_) => Some(
                "a wake that runs a task of its own (a routine) is not built yet: it lands with \
                 the routines row, so nothing can set one today. Set a reminder (`for: owner`) or \
                 a check-in instead",
            ),
            WakeRun::Probe(_) => Some(
                "a wake that runs a probe with no model (a watch) is not built yet: it lands with \
                 the watches row, so nothing can set one today. Set a check-in with `after` and \
                 look when it fires instead",
            ),
        }
    }
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// How many wakes of each kind the daemon may hold (`[kernel] max_wakes`,
/// `max_reminders`, `max_routines`, `max_watches`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeLimits {
    /// The furthest ahead a wake may be set, in ms (`max_wake_ahead_days`).
    pub max_ahead_ms: u64,
    /// Check-ins one conversation may hold at once.
    pub max_wakes: usize,
    /// Reminders the daemon may hold at once, over every conversation.
    pub max_reminders: usize,
    /// Routines, declared for their row.
    pub max_routines: usize,
    /// Watches, declared for their row.
    pub max_watches: usize,
}

impl Default for WakeLimits {
    fn default() -> Self {
        Self {
            max_ahead_ms: 365 * 86_400_000,
            max_wakes: 5,
            max_reminders: 100,
            max_routines: 50,
            max_watches: 20,
        }
    }
}

impl WakeLimits {
    /// The daemon-wide cap of a purpose; none for a check-in, whose cap is
    /// per conversation (`max_wakes`).
    pub fn daemon_cap(&self, p: Purpose) -> Option<usize> {
        match p {
            Purpose::Check => None,
            Purpose::Remind => Some(self.max_reminders),
            Purpose::Routine => Some(self.max_routines),
            Purpose::Watch => Some(self.max_watches),
        }
    }
}

/// A wake to add: what a tool or an owner's verb asks for.
#[derive(Debug, Clone, Default)]
pub struct NewWake {
    pub due_at_ms: u64,
    pub note: String,
    /// Where its session posted when it was set.
    pub target: Option<String>,
    pub repeat: Option<crate::repeat::Repeat>,
    pub purpose: Purpose,
    pub run: WakeRun,
}
