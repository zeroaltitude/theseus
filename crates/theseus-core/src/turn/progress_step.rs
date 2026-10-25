//! The progress check (theseus-2uy56): a turn that has used much of its time
//! or its spend with no passing check is told to replan, and near its
//! deadline to stop and report, instead of going on paying the same way.
//!
//! At each loop's end, a pure rule ([`decide`]) over what the turn already
//! knows: the share of its deadline used (`clock_step`), the share of its
//! execution's spend limit this turn has spent, and the loops since its last
//! passing check. A loop whose commands (`proc.run`) answered in the loop,
//! none with an error result, passed; one whose command failed failed (a
//! failed read beside a passing command is a pass); a loop with no command,
//! or with a command sent to the background, is no check. Reading is progress
//! until there is something to check (theseus-ikr75): a turn that has not
//! edited yet counts no unchecked loops, and after its first edit a loop that
//! read a path the turn had not read before, every read answering, resets the
//! count like a passing command. The rule:
//!
//! - **Nothing** while a check passed in the last `MIN_LOOPS_WITHOUT_PASS`
//!   loops: work that passes is going well, however long it runs.
//! - **A steer** (once a turn) at `STEER_SHARE` of the deadline, or of the
//!   spend limit when the turn has a deadline and its limit is a notice, not
//!   a guard (a guarded turn ends at its limit anyway): a harness note that
//!   says what was used and that it is not a stop. With a failing check it
//!   asks for the smallest step that can make it pass.
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
//! What the turn holds is a field of its `Checkpoints` (turn.rs is at its line
//! ceiling): the state begins at the turn's first loop, takes the deadline of
//! the turn's own `turn.submit` (`clock_step::take_deadline`), and goes with
//! the turn, however it ends.

use super::clock_step::Deadline;
use super::*;
use crate::fact::progress::TurnProgress;
use std::collections::HashSet;

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
    /// The execution's spend limit is a guard (`spend_limit_mode = "ask"`):
    /// the turn ends at it, so spend alone steers nothing. The spend is still
    /// reported.
    pub guarded: bool,
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
    // Spend steers a turn that has a deadline and a limit that only notifies.
    let spend_steers = s.time.is_some() && !s.guarded;
    let spend = s.spend.unwrap_or(0.0);
    if !steered && (time >= STEER_SHARE || (spend_steers && spend >= STEER_SHARE)) {
        return Verdict::Steer;
    }
    Verdict::Nothing
}

/// What one turn holds of its progress and its clock; begun at its first
/// loop, on the turn (`Checkpoints::progress`), so it ends with the turn.
pub(super) struct State {
    deadline: Option<Deadline>,
    /// The time line last shown (`clock_step`).
    shown: Option<u64>,
    /// The execution's spend limit, read once.
    limit_micros: Option<u64>,
    since_pass: u32,
    failing: u32,
    /// An edit call has answered without error in the turn.
    edited: bool,
    /// The paths the turn has read (keys, no contents), up to `SEEN_MAX`.
    seen: HashSet<String>,
    steered: bool,
    /// The stop was said: the next loop is the last.
    last: bool,
}

impl State {
    /// A loop ended: what its calls did moves the count of loops since a
    /// passing check.
    fn end_loop(&mut self, commands: Commands) {
        let check = commands.check();
        self.edited |= commands.edited;
        let fresh = !commands.read_failed && commands.reads.iter().any(|k| !self.seen.contains(k));
        for k in commands.reads {
            if self.seen.len() < SEEN_MAX {
                self.seen.insert(k);
            }
        }
        match check {
            Some(true) => (self.since_pass, self.failing) = (0, 0),
            Some(false) => {
                (self.since_pass, self.failing) = (self.since_pass + 1, self.failing + 1)
            }
            // Nothing to check before the first edit.
            None if !self.edited => {}
            // New ground, read without error, is progress.
            None if fresh => (self.since_pass, self.failing) = (0, 0),
            None => self.since_pass += 1,
        }
    }
}

/// The read keys a turn remembers: past them nothing new is coverage.
const SEEN_MAX: usize = 4096;
/// The tools that change a file, and the read-only ones that open a path.
const EDITS: [&str; 3] = ["fs.write", "fs.edit", "fs.patch"];
const READS: [&str; 4] = ["fs.read", "fs.list", "fs.glob", "fs.grep"];

/// What a loop's commands did, set as their calls answer
/// (`Checkpoints::answered`) and taken at the loop's end. Only commands: a
/// failed read beside a passing command is a pass, and a command sent to the
/// background is no check, whatever the turn's other jobs did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Commands {
    /// A command answered in the loop.
    answered: bool,
    /// An answered command's result was an error.
    failed: bool,
    /// A command went to the background.
    background: bool,
    /// An edit call answered without error.
    edited: bool,
    /// A read-only call's result was an error.
    read_failed: bool,
    /// What the loop's answered reads opened.
    reads: Vec<String>,
}

impl Commands {
    /// A call `u` ended as `outcome`: `note`, and what it edited or opened.
    pub(super) fn note_use(&mut self, u: &ToolUse, outcome: Option<bool>) {
        self.note(&u.name, outcome);
        let Some(error) = outcome else { return };
        let is = |canon: &str| u.name == canon || u.name == theseus_tools::wire_name(canon);
        if !error && EDITS.iter().any(|t| is(t)) {
            self.edited = true;
        }
        if let Some(canon) = READS.iter().find(|t| is(t)) {
            if error {
                self.read_failed = true;
                return;
            }
            let field = |k: &str| u.input.get(k).and_then(|v| v.as_str()).unwrap_or("");
            let path = if field("path").is_empty() {
                "."
            } else {
                field("path")
            };
            self.reads
                .push(format!("{canon}:{path}:{}", field("pattern")));
        }
    }

    /// A call of `tool` ended as `outcome`: `Some(is_error)` when it answered,
    /// `None` when it went to the background.
    pub(super) fn note(&mut self, tool: &str, outcome: Option<bool>) {
        if !is_command(tool) {
            return;
        }
        match outcome {
            Some(error) => (self.answered, self.failed) = (true, self.failed | error),
            None => self.background = true,
        }
    }

    /// The loop's check: `Some(passed)` when a command answered and none went
    /// to the background, else none.
    fn check(&self) -> Option<bool> {
        (self.answered && !self.background).then_some(!self.failed)
    }
}

/// `t`'s state, begun at its first use in the turn: the deadline its
/// `turn.submit` gave it, if any.
fn with<R>(t: &mut Turn<'_>, f: impl FnOnce(&mut State) -> R) -> R {
    if t.checkpoint.progress.is_none() {
        let limit_micros =
            t.tc.kernel
                .execution(t.tc.execution_id)
                .ok()
                .flatten()
                .map(|e| e.budget.limit_micros)
                .filter(|l| *l > 0);
        t.checkpoint.progress = Some(State {
            deadline: super::clock_step::take_deadline(t.tc.session_id, t.arrived),
            shown: None,
            limit_micros,
            since_pass: 0,
            failing: 0,
            edited: false,
            seen: HashSet::new(),
            steered: false,
            last: false,
        });
    }
    f(t.checkpoint.progress.as_mut().expect("begun above"))
}

/// The turn's deadline and the time line last shown, when it has one.
pub(super) fn clock(t: &mut Turn<'_>) -> Option<(Deadline, Option<u64>)> {
    with(t, |s| s.deadline.map(|d| (d, s.shown)))
}

/// The time line `bucket` was shown.
pub(super) fn shown(t: &mut Turn<'_>, bucket: u64) {
    with(t, |s| s.shown = Some(bucket));
}

/// Whether a tool is a command, whose result is a check.
fn is_command(name: &str) -> bool {
    name == "proc.run" || name == theseus_tools::wire_name("proc.run")
}

impl TurnRunner {
    /// After loop `i`'s results, before the checkpoint takes the loop's
    /// failure: whether the turn goes on. `going_on` is the advancer's word.
    pub(super) fn progress(&self, t: &mut Turn<'_>, i: u32, going_on: bool) -> Result<bool> {
        let guarded = self.cfg.kernel.spend_limit_mode == crate::config::SpendLimitMode::Ask;
        let commands = std::mem::take(&mut t.checkpoint.commands);
        let cost = t.cost;
        let now = Instant::now();
        let (signals, steered, last, deadline) = with(t, |s| {
            s.end_loop(commands);
            let spend = s.limit_micros.zip(cost).map(|(l, c)| c * 1e6 / l as f64);
            let signals = Signals {
                time: s.deadline.map(|d| d.used(now)),
                spend,
                since_pass: s.since_pass,
                failing: s.failing,
                guarded,
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
        0 => format!("no check has run in its last {} loops", s.since_pass),
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
        _ if s.failing > 0 => format!(
            "[The harness: this turn has used {used}, and {checks}. This is not a stop: if work \
             is left, keep going. Say in a line what is left, then take the smallest step that \
             can make the failing check pass.]"
        ),
        _ => format!(
            "[The harness: this turn has used {used}, and {checks}. This is not a stop: if work \
             is left, keep going. Say in a line what is left, then go on with the next item.]"
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
            guarded: false,
        }
    }

    /// The same under a guard (`spend_limit_mode = "ask"`).
    fn guarded(mut sig: Signals) -> Signals {
        sig.guarded = true;
        sig
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
            (s(Some(0.1), Some(0.5), 4, 2), false, Steer),
            (s(Some(0.3), Some(0.5), 3, 0), false, Steer),
            // Spend alone steers neither a turn with no deadline nor a
            // guarded one; the time share still does.
            (s(None, Some(0.5), 4, 2), false, Nothing),
            (s(None, Some(0.9), 40, 0), false, Nothing),
            (guarded(s(Some(0.1), Some(0.7), 4, 2)), false, Nothing),
            (guarded(s(Some(0.5), Some(0.1), 3, 0)), false, Steer),
            (guarded(s(Some(0.6), Some(0.9), 3, 3)), false, Steer),
            (guarded(s(Some(0.85), None, 3, 0)), false, Stop),
            (guarded(s(Some(0.9), Some(0.1), 7, 7)), true, Stop),
            (s(Some(0.2), Some(0.7), 6, 1), false, Steer),
            // Once a turn.
            (s(Some(0.7), Some(0.7), 6, 6), true, Nothing),
            // The stop at 0.85 of the time, steered or not.
            (s(Some(0.85), None, 3, 3), false, Stop),
            (s(Some(0.9), Some(0.1), 7, 0), true, Stop),
            // Spend alone never stops.
            (s(None, Some(3.0), 30, 30), true, Nothing),
            (s(Some(0.1), Some(3.0), 30, 30), false, Steer),
            (guarded(s(Some(0.1), Some(3.0), 30, 30)), false, Nothing),
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
        // A failing check: not a stop, and the smallest step is about it.
        let sig = s(Some(0.6), None, 5, 3);
        assert_eq!(
            note(Verdict::Steer, "9 of its 15 minutes", &sig),
            "[The harness: this turn has used 9 of its 15 minutes, and no check has passed in \
             its last 5 loops (3 failed). This is not a stop: if work is left, keep going. Say \
             in a line what is left, then take the smallest step that can make the failing \
             check pass.]"
        );
        // Checks that pass, or none run: not a stop, and no smallest step.
        let sig = s(Some(0.6), None, 5, 0);
        let passing = note(Verdict::Steer, "9 of its 15 minutes", &sig);
        assert_eq!(
            passing,
            "[The harness: this turn has used 9 of its 15 minutes, and no check has run in its \
             last 5 loops. This is not a stop: if work is left, keep going. Say in a line what \
             is left, then go on with the next item.]"
        );
        assert!(passing.contains("This is not a stop") && !passing.contains("smallest step"));
        let sig = s(Some(0.9), None, 8, 0);
        assert!(note(Verdict::Stop, "13 of its 15 minutes", &sig).contains(
            "no check has run in its last 8 loops. Stop here: in your next answer make no \
             tool calls"
        ));
        assert!(is_command("proc_run") && is_command("proc.run") && !is_command("fs_read"));
    }

    /// A loop's check comes from its commands' results alone (theseus-rmjqr).
    #[test]
    fn a_loops_check_comes_from_its_commands_alone() {
        let (done, error) = (Some(false), Some(true));
        let loop_of = |calls: &[(&str, Option<bool>)]| {
            let mut c = Commands::default();
            for (tool, outcome) in calls {
                c.note(tool, *outcome);
            }
            c.check()
        };
        // A passing command beside a failed read is a pass.
        assert_eq!(
            loop_of(&[("proc_run", done), ("fs_read", error)]),
            Some(true)
        );
        // A failed command is a failed check, wherever it sits.
        assert_eq!(
            loop_of(&[("proc_run", error), ("fs_read", done)]),
            Some(false)
        );
        assert_eq!(
            loop_of(&[("proc.run", done), ("proc_run", error)]),
            Some(false)
        );
        // No command, no check.
        assert_eq!(loop_of(&[("fs_read", done), ("fs_read", error)]), None);
        assert_eq!(loop_of(&[]), None);
        // A command sent to the background is no check, though a job that
        // ended in the same loop left the turn's count unchanged.
        assert_eq!(loop_of(&[("proc_run", None)]), None);
        assert_eq!(loop_of(&[("proc_run", done), ("proc_run", None)]), None);
    }

    /// Reading is progress until there is something to check (theseus-ikr75).
    #[test]
    fn reading_is_progress_until_there_is_something_to_check() {
        use serde_json::json;
        let call = |tool: &str, path: &str| ToolUse {
            id: "t".into(),
            name: tool.into(),
            input: json!({ "path": path, "pattern": "x" }),
        };
        let loop_of = |calls: &[(&str, &str, bool)]| {
            let mut c = Commands::default();
            for (tool, path, error) in calls {
                c.note_use(&call(tool, path), Some(*error));
            }
            c
        };
        let mut st = State {
            deadline: None,
            shown: None,
            limit_micros: None,
            since_pass: 0,
            failing: 0,
            edited: false,
            seen: HashSet::new(),
            steered: false,
            last: false,
        };
        // Before any edit, reads count nothing, new ground or not.
        for _ in 0..6 {
            st.end_loop(loop_of(&[("fs_read", "a.rs", false)]));
        }
        assert_eq!(st.since_pass, 0);
        // A failed edit is no edit; an answered one is.
        let c = loop_of(&[("fs_edit", "a.rs", true)]);
        assert!(!c.edited);
        st.end_loop(c);
        assert_eq!((st.edited, st.since_pass), (false, 0));
        st.end_loop(loop_of(&[("fs_edit", "a.rs", false)]));
        assert_eq!(
            (st.edited, st.since_pass),
            (true, 1),
            "the edit's own loop is unchecked"
        );
        // Re-reading what was read is not coverage; a new path is.
        st.end_loop(loop_of(&[("fs_read", "a.rs", false)]));
        assert_eq!(st.since_pass, 2);
        st.end_loop(loop_of(&[("fs_read", "b.rs", false)]));
        assert_eq!(st.since_pass, 0);
        // A new path beside a read that failed is not coverage either.
        st.end_loop(loop_of(&[
            ("fs_read", "c.rs", false),
            ("fs_read", "gone.rs", true),
        ]));
        assert_eq!(st.since_pass, 1);
        // The same path searched for something else is new ground.
        st.end_loop(loop_of(&[("fs_grep", "b.rs", false)]));
        assert_eq!(st.since_pass, 0);
        // A failed command is a failed check, whatever was read.
        let mut c = loop_of(&[("fs_read", "d.rs", false)]);
        c.note("proc_run", Some(true));
        st.end_loop(c);
        assert_eq!((st.since_pass, st.failing), (1, 1));
    }
}
