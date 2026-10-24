//! The progress check (theseus-2uy56): a turn that has used much of its time
//! or its spend with no passing check is told to replan, and near its
//! deadline to stop and report, instead of going on paying the same way.
//!
//! At each loop's end, a pure rule ([`decide`]) over what the turn already
//! knows: the share of its deadline used (`clock_step`), the share of its
//! execution's spend limit this turn has spent, and the loops since its last
//! passing check. A loop that ran a command (`proc.run`) that answered in the
//! loop, with no tool result an error, passed; one whose command failed
//! failed; a loop with no command, or one sent to the background, is no
//! check. The rule:
//!
//! - **Nothing** while a check passed in the last `MIN_LOOPS_WITHOUT_PASS`
//!   loops: work that passes is going well, however long it runs.
//! - **A steer** (once a turn) at `STEER_SHARE` of the deadline or of the
//!   spend limit: a harness note that says what was used and what has not
//!   passed, and asks the model to replan or narrow.
//! - **A stop** at `STOP_SHARE` of the deadline (time only: under the owner's
//!   notify rule spend never stops a turn): a harness note that asks for a
//!   report with no tool calls, and the next loop is the turn's last (its
//!   stop reason `progress_stop`).
//!
//! Each steer and stop is a `turn.progress` row with the numbers that fired
//! it. The notes are harness nodes appended after the loop's results, outside
//! the cached prefix. A turn with no deadline and little spend pays a map
//! lookup a loop.
//!
//! What the turn holds is kept here by execution, not on `Turn` (turn.rs is
//! at its line ceiling): a turn's entry starts at its first loop, is dropped
//! at its end, and is replaced by the execution's next turn.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use super::clock_step::Deadline;
use super::*;
use crate::fact::progress::TurnProgress;

/// The notes' author on their nodes.
pub const PROGRESS_AUTHOR: &str = "harness:progress";
/// The stop reason of a turn the check stopped.
pub const PROGRESS_STOP: &str = "progress_stop";

/// A check passed in this many of the last loops: the work is going well,
/// and nothing fires. Three loops is one try, one fix and one rerun.
pub const MIN_LOOPS_WITHOUT_PASS: u32 = 3;
/// The share of its deadline, or of its spend limit, at which a turn with no
/// recent passing check is told to replan: half leaves time to change course,
/// and earlier would nag work that is only long.
pub const STEER_SHARE: f64 = 0.5;
/// The share of its deadline at which such a turn is told to stop and report:
/// at 15 minutes, about 2 left, room for one loop to write its report rather
/// than be cut off at the limit with nothing said.
pub const STOP_SHARE: f64 = 0.85;

/// What the rule reads at a loop's end.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Signals {
    /// The share of the turn's deadline used; none without one.
    pub time: Option<f64>,
    /// The share of its execution's spend limit this turn has spent; none
    /// without a limit or a known price.
    pub spend: Option<f64>,
    /// Loops since the last passing check (all of them, with none yet).
    pub since_pass: u32,
    /// Of those, the loops whose command failed.
    pub failing: u32,
}

/// What the rule decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Nothing,
    Steer,
    Stop,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::Steer => "steer",
            Self::Stop => "stop",
        }
    }
}

/// The rule: `steered` is whether the turn was steered already.
pub fn decide(s: &Signals, steered: bool) -> Verdict {
    if s.since_pass < MIN_LOOPS_WITHOUT_PASS {
        return Verdict::Nothing;
    }
    let time = s.time.unwrap_or(0.0);
    if time >= STOP_SHARE {
        return Verdict::Stop;
    }
    if !steered && (time >= STEER_SHARE || s.spend.unwrap_or(0.0) >= STEER_SHARE) {
        return Verdict::Steer;
    }
    Verdict::Nothing
}

/// What one turn holds of its progress and its clock.
struct State {
    turn_id: String,
    deadline: Option<Deadline>,
    /// The time line last shown (`clock_step`).
    shown: Option<u64>,
    /// The execution's spend limit, read once.
    limit_micros: Option<u64>,
    since_pass: u32,
    failing: u32,
    /// The turn's background jobs at the last loop's end.
    background: usize,
    steered: bool,
    /// The stop was said: the next loop is the last.
    last: bool,
}

/// Each execution's running turn's state.
static STATES: Mutex<Option<HashMap<String, State>>> = Mutex::new(None);

/// `t`'s state, begun at its first use in the turn.
fn with<R>(t: &Turn<'_>, f: impl FnOnce(&mut State) -> R) -> R {
    let mut map = STATES.lock().unwrap_or_else(PoisonError::into_inner);
    let map = map.get_or_insert_with(HashMap::new);
    let fresh = map
        .get(t.tc.execution_id)
        .is_none_or(|s| s.turn_id != t.tc.turn_id);
    if fresh {
        let limit_micros =
            t.tc.kernel
                .execution(t.tc.execution_id)
                .ok()
                .flatten()
                .map(|e| e.budget.limit_micros)
                .filter(|l| *l > 0);
        map.insert(
            t.tc.execution_id.to_string(),
            State {
                turn_id: t.tc.turn_id.to_string(),
                deadline: super::clock_step::deadline_of(t.tc.session_id),
                shown: None,
                limit_micros,
                since_pass: 0,
                failing: 0,
                background: t.background.len(),
                steered: false,
                last: false,
            },
        );
    }
    f(map.get_mut(t.tc.execution_id).expect("inserted above"))
}

fn forget(t: &Turn<'_>) {
    let mut map = STATES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(m) = map.as_mut() {
        m.remove(t.tc.execution_id);
    }
}

/// The turn's deadline and the time line last shown, when it has one. A
/// turn with none begins no state for it.
pub(super) fn clock(t: &Turn<'_>) -> Option<(Deadline, Option<u64>)> {
    if super::clock_step::deadline_of(t.tc.session_id).is_none() && !held(t) {
        return None;
    }
    with(t, |s| s.deadline.map(|d| (d, s.shown)))
}

/// Whether the turn holds its state already: a deadline that passed during
/// the turn still counts for it.
fn held(t: &Turn<'_>) -> bool {
    let map = STATES.lock().unwrap_or_else(PoisonError::into_inner);
    map.as_ref()
        .and_then(|m| m.get(t.tc.execution_id))
        .is_some_and(|s| s.turn_id == t.tc.turn_id)
}

/// The time line `bucket` was shown.
pub(super) fn shown(t: &Turn<'_>, bucket: u64) {
    with(t, |s| s.shown = Some(bucket));
}

/// Whether a tool is a command, whose result is a check.
fn is_command(name: &str) -> bool {
    name == "proc.run" || name == theseus_tools::wire_name("proc.run")
}

impl TurnRunner {
    /// After loop `i`'s results, before the checkpoint takes the loop's
    /// failure: whether the turn goes on. `going_on` is the advancer's word.
    pub(super) fn progress(
        t: &mut Turn<'_>,
        uses: &[ToolUse],
        i: u32,
        going_on: bool,
    ) -> Result<bool> {
        let ran = uses.iter().any(|u| is_command(&u.name));
        let (failed, background) = (t.checkpoint.failed, t.background.len());
        let cost = t.cost;
        let now = Instant::now();
        let (signals, steered, last, deadline) = with(t, |s| {
            let check = (ran && background == s.background).then_some(!failed);
            s.background = background;
            match check {
                Some(true) => (s.since_pass, s.failing) = (0, 0),
                Some(false) => (s.since_pass, s.failing) = (s.since_pass + 1, s.failing + 1),
                None => s.since_pass += 1,
            }
            let spend = s.limit_micros.zip(cost).map(|(l, c)| c * 1e6 / l as f64);
            let signals = Signals {
                time: s.deadline.map(|d| d.used(now)),
                spend,
                since_pass: s.since_pass,
                failing: s.failing,
            };
            (signals, s.steered, s.last, s.deadline)
        });
        if last || !going_on {
            if last && (going_on || t.stop_reason == "no_tool_calls") {
                t.stop_reason = PROGRESS_STOP.into();
                t.record(&fact::turn::LoopCut {
                    decision: PROGRESS_STOP,
                });
            }
            forget(t);
            return Ok(false);
        }
        let verdict = decide(&signals, steered);
        if verdict == Verdict::Nothing {
            return Ok(true);
        }
        let limit = with(t, |s| {
            s.steered = true;
            s.last = verdict == Verdict::Stop;
            s.limit_micros
        });
        let used = used_words(&signals, deadline, now, cost, limit);
        let text = note(verdict, &used, &signals);
        t.record(&TurnProgress {
            decision: verdict.word(),
            loop_index: i,
            signals: &signals,
            elapsed_ms: deadline.map(|d| now.saturating_duration_since(d.from).as_millis() as u64),
            deadline_ms: deadline
                .map(|d| d.at.saturating_duration_since(d.from).as_millis() as u64),
            spent_usd: cost,
            limit_usd: limit.map(micros_to_usd),
        });
        let node = Node::relayed(
            t.tc.session_id,
            Some(t.tc.turn_id),
            crate::node::Origin::Harness,
            PROGRESS_AUTHOR,
            &text,
        );
        t.tc.store.append(&[node.record()?])?;
        t.tc.node_written(&node);
        Ok(true)
    }
}

/// What the turn has used, in words: its minutes, its spend, or both, each
/// only when its share is past the steer.
fn used_words(
    s: &Signals,
    deadline: Option<Deadline>,
    now: Instant,
    cost: Option<f64>,
    limit: Option<u64>,
) -> String {
    let mut parts = Vec::new();
    if let (Some(d), Some(share)) = (deadline, s.time) {
        if share >= STEER_SHARE {
            let used = now.saturating_duration_since(d.from).as_secs() / 60;
            parts.push(format!("{used} of its {} minutes", d.minutes()));
        }
    }
    if let (Some(c), Some(l), Some(share)) = (cost, limit, s.spend) {
        if share >= STEER_SHARE {
            parts.push(format!(
                "{} of its {} spend limit",
                narrative::dollars((c * 1e6) as u64),
                narrative::dollars(l)
            ));
        }
    }
    parts.join(" and ")
}

/// The note the model reads: short, plain, and about no tool or language.
fn note(verdict: Verdict, used: &str, s: &Signals) -> String {
    let checks = match s.failing {
        0 => format!("no check has passed in its last {} loops", s.since_pass),
        f => format!(
            "no check has passed in its last {} loops ({f} failed)",
            s.since_pass
        ),
    };
    match verdict {
        Verdict::Stop => format!(
            "[The harness: this turn has used {used}, and {checks}. Stop here: in your next \
             answer make no tool calls, and report what is done, what is not, and why. That \
             answer ends the turn.]"
        ),
        _ => format!(
            "[The harness: this turn has used {used}, and {checks}. Replan or narrow: say what \
             is left and what has not passed, then take the smallest step that can pass.]"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(time: Option<f64>, spend: Option<f64>, since_pass: u32, failing: u32) -> Signals {
        Signals {
            time,
            spend,
            since_pass,
            failing,
        }
    }

    /// The table: time share × spend share × loops since a passing check ×
    /// failing loops, and whether the turn was steered already.
    #[test]
    fn the_rule_steers_then_stops_only_without_a_recent_passing_check() {
        use Verdict::{Nothing, Steer, Stop};
        let rows: &[(Signals, bool, Verdict)] = &[
            // Nothing fires early, whatever the checks.
            (s(Some(0.1), Some(0.1), 9, 9), false, Nothing),
            (s(Some(0.49), Some(0.49), 9, 9), false, Nothing),
            // The steer at half the time, or half the spend.
            (s(Some(0.5), None, 3, 0), false, Steer),
            (s(Some(0.6), Some(0.1), 5, 5), false, Steer),
            (s(None, Some(0.5), 4, 2), false, Steer),
            (s(Some(0.2), Some(0.7), 6, 1), false, Steer),
            // Once a turn.
            (s(Some(0.7), Some(0.7), 6, 6), true, Nothing),
            // The stop at 0.85 of the time, steered or not.
            (s(Some(0.85), None, 3, 3), false, Stop),
            (s(Some(0.9), Some(0.1), 7, 0), true, Stop),
            // Spend alone never stops.
            (s(None, Some(3.0), 30, 30), true, Nothing),
            (s(Some(0.1), Some(3.0), 30, 30), false, Steer),
            // A check passed in the last loops: nothing, at any share.
            (s(Some(0.99), Some(5.0), 0, 0), false, Nothing),
            (s(Some(0.9), Some(0.9), 1, 1), false, Nothing),
            (s(Some(0.9), Some(0.9), 2, 2), true, Nothing),
            // No deadline and no limit: nothing to measure.
            (s(None, None, 50, 50), false, Nothing),
        ];
        for (signals, steered, want) in rows {
            assert_eq!(
                decide(signals, *steered),
                *want,
                "{signals:?} steered={steered}"
            );
        }
    }

    #[test]
    fn the_notes_say_what_was_used_and_what_has_not_passed() {
        let sig = s(Some(0.6), None, 5, 3);
        assert_eq!(
            note(Verdict::Steer, "9 of its 15 minutes", &sig),
            "[The harness: this turn has used 9 of its 15 minutes, and no check has passed in \
             its last 5 loops (3 failed). Replan or narrow: say what is left and what has not \
             passed, then take the smallest step that can pass.]"
        );
        let sig = s(Some(0.9), None, 8, 0);
        assert!(note(Verdict::Stop, "13 of its 15 minutes", &sig).contains(
            "no check has passed in its last 8 loops. Stop here: in your next answer make no \
             tool calls"
        ));
        assert!(is_command("proc_run") && is_command("proc.run") && !is_command("fs_read"));
    }
}
