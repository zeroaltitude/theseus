//! A deferred task (theseus-rcgph; north-star horizon HZ3): `task.create` with
//! `start_at` or `start_after` opens the task asleep, and it starts then.
//!
//! - **Asleep.** `Kernel::open_task_at` writes the task, its session and brief,
//!   and its carve in the one frame a task opens in, with the execution
//!   `Waiting{DueAt}`: no turn runs, and the parent cannot spend the carve
//!   meanwhile. The due scan queues it (`wakes::due_now`, the driver's tick
//!   and the heartbeat), so a restart keeps it, and a time that passed while
//!   the daemon was down starts it on the first scan after.
//! - **The start.** Its first turn is the task's ordinary first turn, and its
//!   catch-up writes one node first (`turn/deferred_step.rs`), the model's
//!   `Deferred task, set <when set> for <due>; started <now>`, with how late
//!   it ran when it ran late. The times are META `task.deferred.<session>`,
//!   written in the open's frame and marked started in the node's.
//! - **Seen.** `TaskInfo.starts_at_ms` is derived from the execution while it
//!   sleeps (`starts_at`), and the board says `▷ starts Fri 09:00`.
//! - **Cancelled** like any task: its carve is released, no turn runs.

use serde::{Deserialize, Serialize};
use theseus_kernel::{ExecState, Execution, Wake};
use theseus_store::{kinds, NewRecord};

use crate::wake::{local, parse_after, parse_at, span, LATE_AFTER_MS, MAX_AHEAD_MS, MIN_AHEAD_MS};

/// The times of a deferred task, kept in the META record `key` names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deferred {
    pub set_at_ms: u64,
    pub due_at_ms: u64,
    /// When its first turn took it: the note is written once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<u64>,
}

/// The META key of a task session's times.
pub fn key(task_session: &str) -> String {
    format!("task.deferred.{task_session}")
}

/// The record that holds `d` for `task_session`.
pub fn record(task_session: &str, d: &Deferred) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&key(task_session)), d)
}

/// When a task asked to start at `at` or `after` starts, from `now_ms`, or why
/// it cannot: the same parse and the same bounds as `wake.at` (`MIN_AHEAD_MS`,
/// `MAX_AHEAD_MS`).
pub fn due(at: Option<&str>, after: Option<&str>, now_ms: u64) -> Result<u64, String> {
    let named = |e: String| {
        e.replace("`after`", "`start_after`")
            .replace("`at`", "`start_at`")
    };
    let due = match (at, after) {
        (Some(_), Some(_)) => return Err("give `start_at` or `start_after`, not both".into()),
        (_, Some(a)) => now_ms.saturating_add(parse_after(a).map_err(named)?),
        (Some(at), None) => {
            let due = parse_at(at).map_err(named)?;
            if due <= now_ms {
                return Err(format!(
                    "`start_at` ({at}) is {} ago; it is {} now. Give a later time, or use \
                     `start_after`",
                    span(now_ms - due),
                    local(now_ms).full()
                ));
            }
            due
        }
        (None, None) => return Err("give `start_at` or `start_after`".into()),
    };
    let ahead = due.saturating_sub(now_ms);
    if ahead < MIN_AHEAD_MS {
        return Err(format!(
            "a task must start at least {} from now",
            span(MIN_AHEAD_MS)
        ));
    }
    if ahead > MAX_AHEAD_MS {
        return Err(format!(
            "a task may be set to start at most {} days from now, and this one is {}",
            MAX_AHEAD_MS / 86_400_000,
            span(ahead)
        ));
    }
    Ok(due)
}

/// When `e` starts, while it sleeps: a task that has taken no turn and waits
/// on its due time.
pub fn starts_at(e: &Execution) -> Option<u64> {
    match (&e.wake, e.state, e.turns, e.parent.is_some()) {
        (Some(Wake::DueAt { at_ms }), ExecState::Waiting, 0, true) => Some(*at_ms),
        _ => None,
    }
}

/// `Fri 09:00` on the daemon's clock.
pub(crate) fn day_hm(ms: u64) -> String {
    let l = local(ms);
    format!("{} {}", l.weekday(), l.hm())
}

/// The note the first turn reads: when the task was set, for when, and when
/// it started, with how late it ran when it ran late and whether the daemon
/// was down at the due time.
pub fn header(d: &Deferred, started_ms: u64, while_down: bool) -> String {
    let l = local(started_ms);
    let late_ms = started_ms.saturating_sub(d.due_at_ms);
    let late = if late_ms > LATE_AFTER_MS {
        let why = if while_down {
            ": the daemon was not running then"
        } else {
            ""
        };
        format!(" ({} late{why})", span(late_ms))
    } else {
        String::new()
    };
    format!(
        "Deferred task, set {} for {}; started {} {}{late}. Start the work in your brief now.",
        day_hm(d.set_at_ms),
        day_hm(d.due_at_ms),
        l.weekday(),
        l.hms(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_000_000_000;

    #[test]
    fn the_start_takes_wake_ats_bounds_in_words() {
        assert_eq!(due(None, Some("2m"), NOW), Ok(NOW + 120_000));
        assert_eq!(due(None, Some("1s"), NOW), Ok(NOW + 1_000));
        let e = due(None, Some("999ms"), NOW).unwrap_err();
        assert!(e.contains("`start_after`"), "{e}");
        let e = due(None, Some("31d"), NOW).unwrap_err();
        assert!(e.contains("at most 30 days from now"), "{e}");
        assert!(due(None, Some("30d"), NOW).is_ok());
        let e = due(Some("2026-01-01T00:00:00Z"), None, NOW).unwrap_err();
        assert!(e.contains("`start_at`") && e.contains("ago"), "{e}");
        let e = due(Some("nope"), None, NOW).unwrap_err();
        assert!(e.contains("`start_at`") && !e.contains("`at`"), "{e}");
        assert!(due(Some("2026-10-14T00:00:00Z"), Some("1h"), NOW).is_err());
        assert!(due(None, None, NOW).is_err());
    }

    #[test]
    fn the_header_says_how_late_it_ran() {
        let d = Deferred {
            set_at_ms: NOW,
            due_at_ms: NOW + 120_000,
            started_ms: None,
        };
        let on_time = header(&d, NOW + 121_000, false);
        assert!(on_time.starts_with("Deferred task, set "), "{on_time}");
        assert!(!on_time.contains("late"), "{on_time}");
        let late = header(&d, NOW + 120_000 + 3_600_000, true);
        assert!(
            late.contains("1 h late: the daemon was not running then"),
            "{late}"
        );
    }
}
