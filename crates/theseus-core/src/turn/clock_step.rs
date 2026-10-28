//! The turn's clock (theseus-yct32): a turn given a deadline (`turn.submit`'s
//! `deadline_secs`, `theseus ask --deadline`) is told the time it has left,
//! at the start of each loop whose whole minutes left changed since the last
//! line, and at its first: "[The harness: 9 minutes left of this turn's
//! 15.]". At most one line a minute, never one a loop. The line is a harness
//! node appended to the session, after every cache breakpoint, so the cached
//! prefix never changes for it; a turn with no deadline adds nothing.
//!
//! The deadline belongs to the turn its `turn.submit` started. It is kept in
//! memory by session and the request's arrival, and the turn whose arrival it
//! is takes it at its first loop (`take_deadline`, which drops the entry):
//! any other turn of the session (a nudge, a wake, a task's resume, an
//! approval's continuation) has none, and so has a restarted daemon's (it
//! fails quiet). Entries are keyed by session and arrival, so a later submit
//! of the session (a steer, a queued submit) never takes or clears an earlier
//! turn's deadline; a submit that joins a running turn gives that turn no
//! deadline of its own. The progress check (`progress_step.rs`) reads the same
//! deadline for its time share.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use super::*;

/// The time line's author on its node.
pub const CLOCK_AUTHOR: &str = "harness:clock";

/// A turn's deadline: when its request arrived, and when its time is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    pub from: Instant,
    pub at: Instant,
}

impl Deadline {
    /// The share of its time used at `now`, 0 to 1 and past.
    pub fn used(&self, now: Instant) -> f64 {
        let whole = self.at.saturating_duration_since(self.from).as_secs_f64();
        match whole > 0.0 {
            true => now.saturating_duration_since(self.from).as_secs_f64() / whole,
            false => 1.0,
        }
    }

    /// Its whole length in minutes, rounded to the nearest, at least 1.
    pub fn minutes(&self) -> u64 {
        let secs = self.at.saturating_duration_since(self.from).as_secs();
        ((secs + 30) / 60).max(1)
    }

    /// Whole minutes left at `now`; none once its time is up.
    fn left(&self, now: Instant) -> Option<u64> {
        let left = self.at.checked_duration_since(now)?;
        (!left.is_zero()).then_some(left.as_secs() / 60)
    }

    /// The line the model reads at `now`.
    pub fn line(&self, now: Instant) -> String {
        let whole = self.minutes();
        let said = match self.left(now) {
            None => format!("this turn's {whole} minutes are up"),
            Some(0) => format!("less than a minute left of this turn's {whole}"),
            Some(1) => format!("1 minute left of this turn's {whole}"),
            Some(m) => format!("{m} minutes left of this turn's {whole}"),
        };
        format!("[The harness: {said}.]")
    }

    /// What changes the line: the whole minutes left, `u64::MAX` once up.
    fn bucket(&self, now: Instant) -> u64 {
        self.left(now).unwrap_or(u64::MAX)
    }
}

/// Each turn's deadline, keyed by its session and its request's arrival: a
/// later submit of the session (a steer, a queued submit) adds or clears only
/// its own entry, never an earlier turn's.
static DEADLINES: Mutex<Option<HashMap<(String, Instant), Deadline>>> = Mutex::new(None);

/// `session`'s turn that arrived at `arrived` has `secs` from then (none: no
/// deadline). Deadlines already passed are dropped as a new one is set. No
/// other arrival's entry is touched.
pub fn set_deadline(session: &str, arrived: Instant, secs: Option<u64>) {
    let mut map = DEADLINES.lock().unwrap_or_else(PoisonError::into_inner);
    let map = map.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    map.retain(|_, d| d.at > now);
    let key = (session.to_string(), arrived);
    match secs {
        Some(s) => map.insert(
            key,
            Deadline {
                from: arrived,
                at: arrived + Duration::from_secs(s),
            },
        ),
        None => map.remove(&key),
    };
}

/// The deadline of `session`'s turn that arrived at `arrived`, when it has one
/// that has not passed: the turn that submit started and no other. A reader
/// for the tests; the turn takes its deadline with [`take_deadline`].
#[cfg(test)]
pub(super) fn deadline_of(session: &str, arrived: Instant) -> Option<Deadline> {
    let map = DEADLINES.lock().unwrap_or_else(PoisonError::into_inner);
    map.as_ref()?
        .get(&(session.to_string(), arrived))
        .copied()
        .filter(|d| d.at > Instant::now())
}

/// The moment a helper of a waited fan-out must have reported by, by its session
/// (theseus-z9mze): its task's turns have no submit of their own to carry a deadline, so the
/// call that opened it sets one for its session, and each turn of the session reads it as that
/// turn's own, from the turn's arrival. The progress check's stop then asks a helper for its
/// report before the wait that holds the parent runs out.
static HELPER_DEADLINES: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// `session`'s turns must be done by `at`. Deadlines already passed are dropped as one is set.
pub fn set_helper_deadline(session: &str, at: Instant) {
    let mut map = HELPER_DEADLINES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let map = map.get_or_insert_with(HashMap::new);
    map.retain(|_, t| *t > Instant::now());
    map.insert(session.to_string(), at);
}

/// The helper deadline of `session`, when it has one that has not passed, as a turn that arrived
/// at `arrived` holds it.
fn helper_deadline(session: &str, arrived: Instant) -> Option<Deadline> {
    let map = HELPER_DEADLINES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let at = *map.as_ref()?.get(session)?;
    (at > Instant::now()).then_some(Deadline { from: arrived, at })
}

/// The deadline of the turn that arrived at `arrived`, taken: the entry goes,
/// read and removed under one lock. A helper's session, which no `turn.submit`
/// gave one, has its call's (`set_helper_deadline`), which stays for its later turns.
pub(super) fn take_deadline(session: &str, arrived: Instant) -> Option<Deadline> {
    let taken = {
        let mut map = DEADLINES.lock().unwrap_or_else(PoisonError::into_inner);
        map.as_mut().and_then(|m| {
            m.remove(&(session.to_string(), arrived))
                .filter(|d| d.at > Instant::now())
        })
    };
    let taken = taken.or_else(|| helper_deadline(session, arrived));
    in_force_set(session, taken);
    taken
}

/// The deadline of each session's running turn, as that turn took it (none:
/// the turn has no deadline). A tool that runs in its own task reads it
/// (`in_force`): `task.wait` keeps back the time its turn needs to finish.
static IN_FORCE: Mutex<Option<HashMap<String, Deadline>>> = Mutex::new(None);

fn in_force_set(session: &str, deadline: Option<Deadline>) {
    let mut map = IN_FORCE.lock().unwrap_or_else(PoisonError::into_inner);
    let map = map.get_or_insert_with(HashMap::new);
    match deadline {
        Some(d) => map.insert(session.to_string(), d),
        None => map.remove(session),
    };
}

/// `session`'s turn ended: its deadline is no longer in force, so a later wait
/// of the session never clamps to a stale one.
pub(super) fn turn_ended(session: &str) {
    in_force_set(session, None);
}

/// How long `session`'s running turn has left, when it has a deadline.
pub fn left_in_force(session: &str) -> Option<Duration> {
    let map = IN_FORCE.lock().unwrap_or_else(PoisonError::into_inner);
    let d = map.as_ref()?.get(session)?;
    Some(d.at.saturating_duration_since(Instant::now()))
}

impl TurnRunner {
    /// Before a loop's compile: the time line, when the turn has a deadline
    /// and its whole minutes left changed since the last line it was shown.
    pub(super) fn clock_line(t: &mut Turn<'_>) -> Result<()> {
        let now = Instant::now();
        let Some((deadline, shown)) = super::progress_step::clock(t) else {
            return Ok(());
        };
        if shown == Some(deadline.bucket(now)) {
            return Ok(());
        }
        let node = Node::relayed(
            t.tc.session_id,
            Some(t.tc.turn_id),
            crate::node::Origin::Harness,
            CLOCK_AUTHOR,
            &deadline.line(now),
        );
        t.tc.store.append(&[node.record()?])?;
        t.tc.node_written(&node);
        super::progress_step::shown(t, deadline.bucket(now));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deadline(secs: u64) -> (Deadline, Instant) {
        let from = Instant::now();
        let d = Deadline {
            from,
            at: from + Duration::from_secs(secs),
        };
        (d, from)
    }

    /// The line rounds down to the whole minute, so it changes once a minute.
    #[test]
    fn the_line_says_whole_minutes_left() {
        let (d, from) = deadline(900);
        let at = |s: u64| from + Duration::from_secs(s);
        assert_eq!(
            d.line(at(0)),
            "[The harness: 15 minutes left of this turn's 15.]"
        );
        assert_eq!(
            d.line(at(61)),
            "[The harness: 13 minutes left of this turn's 15.]"
        );
        assert_eq!(d.bucket(at(61)), d.bucket(at(119)), "one line a minute");
        assert_ne!(d.bucket(at(119)), d.bucket(at(121)));
        assert_eq!(
            d.line(at(800)),
            "[The harness: 1 minute left of this turn's 15.]"
        );
        assert_eq!(
            d.line(at(870)),
            "[The harness: less than a minute left of this turn's 15.]"
        );
        assert_eq!(
            d.line(at(900)),
            "[The harness: this turn's 15 minutes are up.]"
        );
        assert!((d.used(at(450)) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_deadline_is_kept_by_session_until_it_passes() {
        let now = Instant::now();
        set_deadline("ses_clock_a", now, Some(600));
        assert_eq!(
            deadline_of("ses_clock_a", now).map(|d| d.minutes()),
            Some(10)
        );
        assert_eq!(deadline_of("ses_clock_b", now), None);
        // A passed one is none.
        let old = now - Duration::from_secs(5);
        set_deadline("ses_clock_b", old, Some(1));
        assert_eq!(deadline_of("ses_clock_b", old), None);
        // The same turn given none clears its own.
        set_deadline("ses_clock_a", now, None);
        assert_eq!(deadline_of("ses_clock_a", now), None);
    }

    /// Only the turn whose submit set it has the deadline, and takes it once.
    #[test]
    fn a_deadline_belongs_to_the_turn_that_arrived_with_it() {
        let now = Instant::now();
        set_deadline("ses_clock_c", now, Some(600));
        let later = now + Duration::from_millis(5);
        assert_eq!(deadline_of("ses_clock_c", later), None, "another arrival");
        assert_eq!(take_deadline("ses_clock_c", later), None);
        assert!(take_deadline("ses_clock_c", now).is_some());
        assert_eq!(take_deadline("ses_clock_c", now), None, "taken");
    }

    /// A turn's end clears the deadline in force, so a later wait of the session
    /// never clamps to it (the stack review's N10).
    #[test]
    fn a_turns_end_clears_the_deadline_in_force() {
        let now = Instant::now();
        set_deadline("ses_clock_e", now, Some(600));
        assert!(take_deadline("ses_clock_e", now).is_some());
        assert!(left_in_force("ses_clock_e").is_some());
        turn_ended("ses_clock_e");
        assert_eq!(left_in_force("ses_clock_e"), None);
    }

    #[test]
    fn a_second_submit_before_the_first_loop_keeps_the_first_turns_deadline() {
        let a1 = Instant::now();
        let a2 = a1 + Duration::from_millis(5);
        set_deadline("ses_clock_d", a1, Some(600));
        set_deadline("ses_clock_d", a2, Some(300));
        assert_eq!(
            take_deadline("ses_clock_d", a1).map(|d| d.minutes()),
            Some(10)
        );
        assert_eq!(
            take_deadline("ses_clock_d", a2).map(|d| d.minutes()),
            Some(5)
        );
    }

    #[test]
    fn a_steer_with_no_deadline_leaves_the_running_turns() {
        let a1 = Instant::now();
        let a2 = a1 + Duration::from_millis(5);
        set_deadline("ses_clock_e", a1, Some(600));
        set_deadline("ses_clock_e", a2, None);
        assert!(take_deadline("ses_clock_e", a1).is_some());
    }
}
