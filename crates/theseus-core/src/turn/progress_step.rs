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
//! or with a command sent to the background, is no check. A build that runs
//! in the background is progress, though (theseus-duzha): a loop that sent a
//! command there, or that waited on or read a job while a job of the turn
//! ran or had just ended, holds both counts where they were, since waiting is
//! what a long build asks of the turn. Reading is progress
//! until there is something to check (theseus-ikr75): a turn that has not
//! edited yet counts no unchecked loops, and after its first edit a loop that
//! read ground the turn had not read before (a path, a pattern, a slice of a
//! file from a new first line), every read answering, resets the count like a
//! passing command. That count is the steer's. The stop keeps its own
//! (`since_check`), which only a passing command resets, because the stop is
//! the only thing that enforces a deadline and reading must not delay it. The
//! rule:
//!
//! - **Nothing** while a check passed in the last `MIN_LOOPS_WITHOUT_PASS`
//!   loops (or, for the steer, new ground was read in them): work that passes
//!   is going well, however long it runs.
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
    /// Loops since the last passing check or, before a turn's first edit or
    /// after a read of new ground, since that (all of them, with none yet).
    /// The steer reads this one.
    pub since_pass: u32,
    /// Loops since the last passing check, however much was read: the
    /// stop reads this one, so reading never postpones a deadline. A loop
    /// that waited on a build in the background holds it where it was
    /// (theseus-duzha): waiting for a job is not a stall.
    pub since_check: u32,
    /// Of `since_pass`, the loops whose command failed.
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
    let time = s.time.unwrap_or(0.0);
    // The stop counts as the base did: nothing else enforces a deadline, so
    // reading (which only quiets the steer) never delays it.
    if s.since_check >= MIN_LOOPS_WITHOUT_PASS && time >= STOP_SHARE {
        return Verdict::Stop;
    }
    if s.since_pass < MIN_LOOPS_WITHOUT_PASS {
        return Verdict::Nothing;
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
    /// Loops since a passing check, reading or not (`Signals::since_check`).
    since_check: u32,
    failing: u32,
    /// An edit call has answered without error in the turn.
    edited: bool,
    /// What the turn has read (keys, no contents), up to `SEEN_MAX`.
    seen: HashSet<String>,
    steered: bool,
    /// The stop was said: the next loop is the last.
    last: bool,
    /// The turn's jobs that ran when a loop last touched one (their
    /// correlation ids): one gone from it has just ended.
    running: HashSet<String>,
    /// The turn edited a file or ran a command (theseus-rqu7j).
    worked: bool,
    /// The turn was sent back once for a doubt its closing message named
    /// (theseus-rqu7j).
    doubted: bool,
}

impl State {
    /// What the rule reads now: the turn's `cost` so far and the clock.
    fn signals(&self, cost: Option<f64>, now: Instant, guarded: bool) -> Signals {
        Signals {
            time: self.deadline.map(|d| d.used(now)),
            spend: self.limit_micros.zip(cost).map(|(l, c)| c * 1e6 / l as f64),
            since_pass: self.since_pass,
            since_check: self.since_check,
            failing: self.failing,
            guarded,
        }
    }

    /// A loop ended: what its calls did moves the count of loops since a
    /// passing check. `live` is the turn's running jobs, read only for a
    /// loop that touched a job (`Commands::touches_jobs`).
    fn end_loop(&mut self, commands: Commands, live: Option<HashSet<String>>) {
        let check = commands.check();
        self.worked |= commands.edited || commands.answered || commands.background;
        // A job running, or one that ended since the last loop that touched
        // one, makes a loop that waited on it or started it progress.
        let held = check.is_none()
            && live.as_ref().is_some_and(|live| {
                commands.background
                    || !live.is_empty()
                    || self.running.iter().any(|c| !live.contains(c))
            });
        if let Some(live) = live {
            self.running = live;
        }
        self.edited |= commands.edited;
        // Past the cap nothing new is coverage, or every read would reset the count.
        let fresh = !commands.read_failed
            && self.seen.len() < SEEN_MAX
            && commands.reads.iter().any(|k| !self.seen.contains(k));
        for k in commands.reads {
            if self.seen.len() < SEEN_MAX {
                self.seen.insert(k);
            }
        }
        // The stop's count: a passing command resets it, and nothing else does.
        self.since_check = match check {
            Some(true) => 0,
            None if held => self.since_check,
            _ => self.since_check + 1,
        };
        match check {
            Some(true) => (self.since_pass, self.failing) = (0, 0),
            Some(false) => {
                (self.since_pass, self.failing) = (self.since_pass + 1, self.failing + 1)
            }
            // Nothing to check before the first edit.
            None if !self.edited => {}
            // New ground, read without error, is progress.
            None if fresh => (self.since_pass, self.failing) = (0, 0),
            // A build in the background, or one just done: nothing lost.
            None if held => {}
            None => self.since_pass += 1,
        }
    }
}

/// The read keys a turn remembers: past them nothing new is coverage.
const SEEN_MAX: usize = 4096;
/// The tools that change a file, and the read-only ones that open a path.
const EDITS: [&str; 3] = ["fs.write", "fs.edit", "fs.patch"];
const READS: [&str; 4] = ["fs.read", "fs.list", "fs.glob", "fs.grep"];
/// The calls that look at a job without changing it (theseus-duzha).
const JOB_CALLS: [&str; 2] = [theseus_tools::jobs::WAIT, theseus_tools::jobs::READ];

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
    /// A `job.wait` or `job.read` answered without error.
    job_touched: bool,
}

impl Commands {
    /// A call `u` ended as `outcome`: `note`, and what it edited or opened.
    pub(super) fn note_use(&mut self, u: &ToolUse, outcome: Option<bool>) {
        self.note(&u.name, outcome);
        let Some(error) = outcome else { return };
        let is = |canon: &str| u.name == canon || u.name == theseus_tools::wire_name(canon);
        if !error && JOB_CALLS.iter().any(|t| is(t)) {
            self.job_touched = true;
        }
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
            // What makes a read new ground: a search's pattern, and a file's
            // slice, named by its first line (a bare read returns from line 1,
            // as `offset=1` does) or its pages. Slicing a long file, as the
            // narrow-read hint asks, covers new ground at each new start.
            let line = u.input.get("offset").and_then(|v| v.as_u64()).unwrap_or(1);
            let what = if *canon == "fs.read" {
                format!("@{}:{}", line.max(1), field("pages"))
            } else {
                field("pattern").to_string()
            };
            self.reads.push(format!("{canon}:{path}:{what}"));
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

    /// The loop started a job in the background or waited on or read one.
    fn touches_jobs(&self) -> bool {
        self.background || self.job_touched
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
            since_check: 0,
            failing: 0,
            edited: false,
            seen: HashSet::new(),
            steered: false,
            last: false,
            running: HashSet::new(),
            worked: false,
            doubted: false,
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
        // Read from the kernel; an unreadable one counts the loop as before.
        let live = commands
            .touches_jobs()
            .then(|| self.running_jobs(t.tc.execution_id, &t.background).ok())
            .flatten();
        let (signals, steered, last, deadline) = with(t, |s| {
            s.end_loop(commands, live);
            (s.signals(cost, now, guarded), s.steered, s.last, s.deadline)
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
        tell(t, verdict.word(), i, (&signals, deadline), &text)?;
        Ok(true)
    }
}

/// A decision the turn's progress made at loop `i`: its `turn.progress` row,
/// with the numbers that fired it, and the harness node `text` after the
/// loop's results.
pub(super) fn tell(
    t: &mut Turn<'_>,
    decision: &'static str,
    i: u32,
    (signals, deadline): (&Signals, Option<Deadline>),
    text: &str,
) -> Result<()> {
    let limit = with(t, |s| s.limit_micros);
    let (cost, now) = (t.cost, Instant::now());
    t.record(&TurnProgress {
        decision,
        loop_index: i,
        signals,
        elapsed_ms: deadline.map(|d| now.saturating_duration_since(d.from).as_millis() as u64),
        deadline_ms: deadline.map(|d| d.at.saturating_duration_since(d.from).as_millis() as u64),
        spent_usd: cost,
        limit_usd: limit.map(micros_to_usd),
    });
    let node = Node::relayed(
        t.tc.session_id,
        Some(t.tc.turn_id),
        crate::node::Origin::Harness,
        PROGRESS_AUTHOR,
        text,
    );
    t.tc.store.append(&[node.record()?])?;
    t.tc.node_written(&node);
    Ok(())
}

/// Whether the turn wrote or edited a file with a file tool: it built
/// something, which is what the deliverable hint (theseus-eb4o4) is for.
pub(super) fn edited(t: &Turn<'_>) -> bool {
    t.checkpoint.progress.as_ref().is_some_and(|s| s.edited)
}

/// A turn that worked and has not been sent back for a doubt yet, with less
/// than `share` of its time and of its spend used (a share it has no measure
/// of counts as none used, so a turn with no deadline and no spend limit is
/// held by the phrase list alone, `close_out_step`): the numbers to tell it
/// with, and the turn is marked sent back, so it is once.
pub(super) fn doubtful(
    t: &mut Turn<'_>,
    guarded: bool,
    share: f64,
) -> Option<(Signals, Option<Deadline>)> {
    let (cost, now) = (t.cost, Instant::now());
    with(t, |s| {
        let signals = s.signals(cost, now, guarded);
        let under = |x: Option<f64>| x.unwrap_or(0.0) < share;
        let go = s.worked && !s.doubted && under(signals.time) && under(signals.spend);
        s.doubted |= go;
        go.then_some((signals, s.deadline))
    })
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
    // A stop fires on the loops since a passing check, reading or not.
    let loops = match verdict {
        Verdict::Stop => s.since_check,
        _ => s.since_pass,
    };
    let checks = match s.failing {
        0 => format!("no check has run in its last {loops} loops"),
        f => format!("no check has passed in its last {loops} loops ({f} failed)"),
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
            since_check: since_pass,
            failing,
            guarded: false,
        }
    }

    /// A turn that read its way to `since_pass` while `since_check` loops
    /// passed no check.
    fn reading(mut sig: Signals, since_check: u32) -> Signals {
        sig.since_check = since_check;
        sig
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
            // Reading quiets the steer, never the stop: a turn that read new
            // ground (nothing unchecked, as far as the steer counts) has
            // still gone `since_check` loops without a passing check.
            (reading(s(Some(0.6), None, 0, 0), 5), false, Nothing),
            (reading(s(Some(0.6), None, 1, 0), 9), false, Nothing),
            (reading(s(Some(0.9), None, 0, 0), 5), false, Stop),
            (reading(s(Some(0.85), None, 2, 0), 3), true, Stop),
            (reading(s(Some(0.9), Some(0.1), 0, 0), 2), false, Nothing),
            (guarded(reading(s(Some(0.9), None, 0, 0), 4)), false, Stop),
            (reading(s(None, Some(3.0), 0, 0), 30), false, Nothing),
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
            since_check: 0,
            failing: 0,
            edited: false,
            seen: HashSet::new(),
            steered: false,
            last: false,
            running: HashSet::new(),
            worked: false,
            doubted: false,
        };
        // Before any edit, reads count nothing, new ground or not.
        for _ in 0..6 {
            st.end_loop(loop_of(&[("fs_read", "a.rs", false)]), None);
        }
        assert_eq!(st.since_pass, 0);
        // A failed edit is no edit; an answered one is.
        let c = loop_of(&[("fs_edit", "a.rs", true)]);
        assert!(!c.edited);
        st.end_loop(c, None);
        assert_eq!((st.edited, st.since_pass), (false, 0));
        st.end_loop(loop_of(&[("fs_edit", "a.rs", false)]), None);
        assert_eq!(
            (st.edited, st.since_pass),
            (true, 1),
            "the edit's own loop is unchecked"
        );
        // Re-reading what was read is not coverage; a new path is.
        st.end_loop(loop_of(&[("fs_read", "a.rs", false)]), None);
        assert_eq!(st.since_pass, 2);
        st.end_loop(loop_of(&[("fs_read", "b.rs", false)]), None);
        assert_eq!(st.since_pass, 0);
        // A new path beside a read that failed is not coverage either.
        st.end_loop(
            loop_of(&[("fs_read", "c.rs", false), ("fs_read", "gone.rs", true)]),
            None,
        );
        assert_eq!(st.since_pass, 1);
        // The same path searched for something else is new ground.
        st.end_loop(loop_of(&[("fs_grep", "b.rs", false)]), None);
        assert_eq!(st.since_pass, 0);
        // A failed command is a failed check, whatever was read.
        let mut c = loop_of(&[("fs_read", "d.rs", false)]);
        c.note("proc_run", Some(true));
        st.end_loop(c, None);
        assert_eq!((st.since_pass, st.failing), (1, 1));
    }

    /// A turn that has edited, with nothing yet read.
    fn edited_state() -> State {
        State {
            deadline: None,
            shown: None,
            limit_micros: None,
            since_pass: 0,
            since_check: 0,
            failing: 0,
            edited: true,
            seen: HashSet::new(),
            steered: false,
            last: false,
            running: HashSet::new(),
            worked: false,
            doubted: false,
        }
    }

    /// One `fs_read` of `path`, the way a call's input names its slice.
    fn slice(path: &str, extra: serde_json::Value) -> Commands {
        let mut input = serde_json::json!({ "path": path });
        if let (Some(into), Some(from)) = (input.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        let mut c = Commands::default();
        c.note_use(
            &ToolUse {
                id: "t".into(),
                name: "fs_read".into(),
                input,
            },
            Some(false),
        );
        c
    }

    /// Reading a long file slice by slice, as the narrow-read hint asks
    /// (theseus-tdkcn), is progress at each new slice; reading the same slice
    /// again is not. A bare read is the slice from line 1, which is what it
    /// returns, so it and `offset=1` are one slice.
    #[test]
    fn each_new_slice_of_a_file_is_new_ground_and_the_same_slice_is_not() {
        use serde_json::json;
        let mut st = edited_state();
        st.end_loop(slice("big.rs", json!({})), None);
        assert_eq!(st.since_pass, 0, "the first slice is new");
        st.end_loop(slice("big.rs", json!({})), None);
        assert_eq!(st.since_pass, 1, "the same bare read is a re-read");
        st.end_loop(slice("big.rs", json!({ "offset": 1, "limit": 400 })), None);
        assert_eq!(st.since_pass, 2, "offset=1 is the bare read's slice");
        st.end_loop(
            slice("big.rs", json!({ "offset": 401, "limit": 400 })),
            None,
        );
        assert_eq!(st.since_pass, 0, "the next slice is new ground");
        st.end_loop(
            slice("big.rs", json!({ "offset": 401, "limit": 100 })),
            None,
        );
        assert_eq!(st.since_pass, 1, "the same start again is a re-read");
        st.end_loop(
            slice("big.rs", json!({ "offset": 801, "limit": 400 })),
            None,
        );
        assert_eq!(st.since_pass, 0);
        st.end_loop(slice("other.rs", json!({})), None);
        assert_eq!(st.since_pass, 0, "another file's first slice is new");
    }

    /// The stop's count moves as the base's did, whatever was read: only a
    /// passing command resets it (theseus-ikr75 join fix 1).
    #[test]
    fn the_stops_count_moves_as_the_base_did_whatever_was_read() {
        use serde_json::json;
        let read = |n: u32| slice(&format!("f{n}.rs"), json!({}));
        let command = |outcome: Option<bool>| {
            let mut c = Commands::default();
            c.note("proc_run", outcome);
            c
        };
        let mut st = edited_state();
        st.edited = false;
        for n in 0..6 {
            st.end_loop(read(n), None);
        }
        assert_eq!(
            (st.since_pass, st.since_check),
            (0, 6),
            "before an edit, reading is free to the steer alone"
        );
        st.edited = true;
        for n in 6..9 {
            st.end_loop(read(n), None);
        }
        assert_eq!(
            (st.since_pass, st.since_check),
            (0, 9),
            "new files reset the steer's count, not the stop's"
        );
        // A loop with no call is unchecked for both; a failed command and a
        // command sent to the background are no pass either.
        st.end_loop(Commands::default(), None);
        assert_eq!((st.since_pass, st.since_check), (1, 10));
        st.end_loop(command(Some(true)), None);
        assert_eq!((st.since_pass, st.since_check), (2, 11));
        st.end_loop(command(None), None);
        assert_eq!((st.since_pass, st.since_check), (3, 12));
        // A passing command resets both.
        st.end_loop(command(Some(false)), None);
        assert_eq!((st.since_pass, st.since_check), (0, 0));
    }

    /// The remembered reads are capped, and past the cap a read is no
    /// coverage: a full set does not turn every unseen key into new ground.
    #[test]
    fn a_full_set_of_reads_does_not_make_every_read_new() {
        let mut st = edited_state();
        st.seen = (0..SEEN_MAX)
            .map(|n| format!("fs.read:f{n}.rs:@1"))
            .collect();
        st.since_pass = 2;
        st.end_loop(slice("unseen.rs", serde_json::json!({})), None);
        assert_eq!(st.since_pass, 3, "the cap is reached, so nothing is new");
        assert_eq!(st.seen.len(), SEEN_MAX);
        // Below the cap the same read is new ground.
        let mut st = edited_state();
        st.since_pass = 2;
        st.end_loop(slice("unseen.rs", serde_json::json!({})), None);
        assert_eq!(st.since_pass, 0);
    }
    /// A call `name` that answered without error.
    fn answered(name: &str) -> Commands {
        let mut c = Commands::default();
        let input = serde_json::json!({ "job": "j1" });
        let u = ToolUse {
            id: "t".into(),
            name: name.into(),
            input,
        };
        c.note_use(&u, Some(false));
        c
    }

    /// The running jobs, as the kernel names them.
    fn live(jobs: &[&str]) -> Option<HashSet<String>> {
        Some(jobs.iter().map(|j| j.to_string()).collect())
    }

    fn counts(st: &State) -> (u32, u32, u32) {
        (st.since_pass, st.since_check, st.failing)
    }

    /// A build in the background is progress (theseus-duzha): a command sent
    /// there, a wait on a running job and a read of one hold both counts where
    /// they were, however many loops go by.
    #[test]
    fn a_job_that_runs_holds_both_counts() {
        let mut st = edited_state();
        st.since_pass = 2;
        st.since_check = 4;
        // The command goes to the background.
        let mut bg = Commands::default();
        bg.note("proc.run", None);
        assert!(bg.touches_jobs());
        st.end_loop(bg, live(&["c1"]));
        assert_eq!(counts(&st), (2, 4, 0));
        // Waits and reads while it runs.
        for name in ["job_wait", "job_read", "job_wait", "job.wait", "job.read"] {
            let c = answered(name);
            assert!(c.touches_jobs(), "{name}");
            st.end_loop(c, live(&["c1"]));
            assert_eq!(counts(&st), (2, 4, 0), "{name}");
        }
        // The loop that sees it ended is held too: the result is news.
        st.end_loop(answered("job_wait"), live(&[]));
        assert_eq!(counts(&st), (2, 4, 0), "newly finished");
        // The next read of the finished job is not.
        st.end_loop(answered("job_read"), live(&[]));
        assert_eq!(counts(&st), (3, 5, 0), "an old job is no news");
    }

    /// What the hold does not touch: a loop with no job call is counted as it
    /// was, a failing command still fails, a passing one still resets, new
    /// ground still resets, and before the first edit nothing counts.
    #[test]
    fn the_hold_leaves_the_rest_of_the_rule_as_it_was() {
        let mut st = edited_state();
        st.since_pass = 2;
        st.since_check = 2;
        // A job runs, but this loop did not touch it.
        st.end_loop(Commands::default(), None);
        assert_eq!(counts(&st), (3, 3, 0), "no job call, no hold");
        // A command that failed beside a read of a running job: it failed.
        let mut c = answered("job_read");
        c.note("proc.run", Some(true));
        st.end_loop(c, live(&["c1"]));
        assert_eq!(counts(&st), (4, 4, 1));
        // New ground beside a running job resets the steer's count only.
        let mut c = slice("a.rs", serde_json::json!({}));
        c.job_touched = true;
        st.end_loop(c, live(&["c1"]));
        assert_eq!(counts(&st), (0, 4, 0));
        // The job ended since: that wait is held once, the next is not.
        st.end_loop(answered("job_wait"), live(&[]));
        assert_eq!(counts(&st), (0, 4, 0));
        st.end_loop(answered("job_wait"), live(&[]));
        assert_eq!(counts(&st), (1, 5, 0));
        // A job call that failed (an unknown job) touches none.
        let mut c = Commands::default();
        c.note_use(
            &ToolUse {
                id: "t".into(),
                name: "job_wait".into(),
                input: serde_json::json!({ "job": "j9" }),
            },
            Some(true),
        );
        assert!(!c.touches_jobs());
        // A passing command resets both, a job running or not.
        let mut c = Commands::default();
        c.note("proc.run", Some(false));
        st.end_loop(c, live(&["c1"]));
        assert_eq!(counts(&st), (0, 0, 0));
        // Before the first edit the steer counts nothing; the stop's count
        // is held with the job.
        let mut st = edited_state();
        st.edited = false;
        st.end_loop(answered("job_wait"), live(&["c1"]));
        assert_eq!(counts(&st), (0, 0, 0));
    }

    /// The feature stays: held loops do not reach the steer or the stop, and
    /// the first loop with no job in it that is no progress counts again, so
    /// the steer and the stop fire when there is genuinely none.
    #[test]
    fn the_steer_and_the_stop_still_fire_when_nothing_progresses() {
        let mut st = edited_state();
        let mut verdicts = Vec::new();
        let at = |st: &State, time: f64| {
            let sig = Signals {
                time: Some(time),
                spend: None,
                since_pass: st.since_pass,
                since_check: st.since_check,
                failing: st.failing,
                guarded: false,
            };
            decide(&sig, st.steered)
        };
        // Ten loops waiting on a build, deep into the deadline: nothing.
        for _ in 0..10 {
            st.end_loop(answered("job_wait"), live(&["c1"]));
            verdicts.push(at(&st, 0.9));
        }
        assert!(
            verdicts.iter().all(|v| *v == Verdict::Nothing),
            "{verdicts:?}"
        );
        // Then loops that touch nothing and pass nothing: the count moves.
        for _ in 0..2 {
            st.end_loop(Commands::default(), None);
        }
        assert_eq!(at(&st, 0.55), Verdict::Nothing);
        st.end_loop(Commands::default(), None);
        assert_eq!(at(&st, 0.55), Verdict::Steer);
        assert_eq!(at(&st, 0.9), Verdict::Stop);
    }
}
