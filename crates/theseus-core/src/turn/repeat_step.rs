//! A turn that re-plans the same work for ever (`p5kln-p0-unbounded-hold`,
//! row B6 `theseus-2yowd`): the repeat the Advancer ends the turn on.
//!
//! WHY THIS EXISTS. `undo.v1`'s middle verdict, `irreversible_reshapeable`,
//! holds a call and sends the model back to re-plan it. That hold is the point
//! of the feature and it is also the one answer in the product that no cap
//! bounds. `undo.v1.toml`'s own header says so: `on_max_loops` defaults to
//! `notify`, which makes the Advancer's `max_loops` arm unreachable
//! (`advancer.rs`: the arm is `!self.notify && …`); `loop.v1` is dispatched
//! only for a turn that stopped on `no_tool_calls`, which a held loop never
//! reaches; and the progress check's steer arms on an edit that answered
//! WITHOUT error, while a held call's answer is an error to every reader that
//! asks `is_error` (`node::ResultStatus::is_error`). The terminus was the
//! kernel's hard multiple of the spend limit — about ten times it, and some
//! thousands of loops. This module is the repeat detector that was owed before
//! the pack could be wired, and it is worth landing whether or not it ever is:
//! nothing here mentions `undo.v1`, and any hold, refusal or wall that sends
//! the model back to the same plane is bounded by it.
//!
//! WHAT COUNTS AS A REPEAT, AND WHY IT IS NOT "THE SAME CALL TWICE". The card
//! that asked for this said "the commonest identical call repeats N times".
//! Taken literally that ends legitimate turns, and the counter-example is the
//! commonest shape of work there is: a model fixing a test runs `cargo test`
//! with byte-identical argv ten or twenty times in one turn, because between
//! each run it edits a file. So the identity counted here is the whole
//! **loop's** signature — every tool call the loop asked for, by tool name and
//! canonical arguments, in order — and not one call inside it. The fixing loop
//! is `{edit a, cargo test}`, `{edit b, cargo test}`, …, whose signatures all
//! differ; a model re-planning into a hold asks for the same thing again with
//! nothing else beside it, and its signatures are equal. A model that runs
//! `cargo test` alone six times with no edit between is the pathology too, and
//! is ended, correctly.
//!
//! ONLY A LOOP THAT RAN NOTHING COUNTS, and this is the rule that makes the
//! detector safe to have on. A loop whose calls RAN made progress, whatever it
//! asked for and however the calls turned out: the product already has three
//! instruments for a turn that runs the same thing and gets nowhere — the loop
//! notice at each multiple of `max_loops`, the effort checkpoint, and the
//! progress steer — and behind all of them the kernel's hard multiple of the
//! spend limit. Those are the owner's designed behaviour and this card has no
//! business overriding them. So `made_no_progress` is the gate: calls were
//! judged, the turn is not parked on a confirm, and NONE of the calls ran.
//!
//! `Error` IS NOT "NEVER RAN". `node::ResultStatus::Error` means "the tool ran
//! and failed, or its input was invalid", so a command that fails six times
//! running is not this rule's business. Measured, not assumed: counting every
//! identical loop reddened 24 tests in theseus-core, among them
//! `tests_money::a_notifying_session_stops_at_ten_times_its_limit` — the test
//! that encodes the very money terminus the pack's header describes — four
//! `tests_fallback` refusal-streak tests, six `tests_turn_progress` steer
//! tests, the effort and task checkpoints, and
//! `tests_output::the_cores_output_matches_its_golden`. Every one of those is
//! designed behaviour, not a stale expectation, so the detector was narrowed
//! rather than the tests changed.
//!
//! WHAT THE WIRING ROW OWED THIS, AND NOW PAYS: a reversibility hold writes
//! its result as `Declined`, not `Error`, so that this rule sees it. A hold
//! IS a call that never ran, which is what `Declined` means
//! (`node::ResultStatus`: "The call never ran"), and writing it as `Error`
//! would make it indistinguishable from a command that ran and failed. The
//! wiring row (`08f4749e`) left it as `Error`, so for a while the one hazard
//! this module was built for was the one hazard it could not see: a model
//! re-planning the same held call produced `[Error]` every loop,
//! `made_no_progress` was false, and the terminus was the spend stop again.
//! `toolrun::invalid_input`'s hold arm is where that is paid
//! (`p0b-held-loop-repeat-cap`), and
//! `tests_undo::a_model_that_keeps_re_planning_the_same_held_call_ends_the_turn`
//! is the whole-core test that would fail if it were taken back out. Nothing
//! here changed for it: `never_ran` is still `Declined | Cancelled` only, so
//! a genuine `Error` from a tool that really ran still counts as progress.
//!
//! The counter lives on the `Turn`, so it dies with the turn: a turn's repeats
//! are its own, and nothing is stored.

use std::collections::BTreeMap;

use crate::node::ResultStatus;
use crate::provider::ToolUse;

/// The Advancer's reason when a turn ends on a repeat. `Decision::label` makes
/// it `end_turn:repeating`.
pub const REPEATING: &str = "repeating";

/// How many times one loop signature may appear in a turn before the turn
/// ends. The sixth identical loop is the last: single-digit loops, as the
/// card's observable asks, and far enough above an accidental pair that a
/// model which simply asked twice is not cut off.
pub const MAX_REPEATS: u32 = 6;

/// Whether a loop made no progress at all, and so counts toward the repeat
/// cap: calls were judged, the turn is not parked on a confirm, and none of
/// them ran. `outcomes` is one status per judged call, in call order; a call
/// still running in the background is `Background`, which ran.
///
/// A hand-written list of tools that "legitimately repeat" was tried first and
/// deleted: a waiting call (`job.wait` and its siblings) RUNS and returns, so
/// this rule already excludes it, and the list was both redundant and wrong —
/// it held canonical dotted names (`job.wait`) while a `ToolUse`'s name is the
/// provider's wire name (`job_wait`, `theseus_tools::wire_name`), so it never
/// matched anything.
pub fn made_no_progress(outcomes: &[ResultStatus], awaiting: bool) -> bool {
    !awaiting && !outcomes.is_empty() && outcomes.iter().all(|s| never_ran(*s))
}

/// A status that means the tool never ran: the operator declined the call, or
/// it was cancelled. `Error` is deliberately not one — it means the tool ran
/// and failed, or its input was invalid (`node::ResultStatus`) — and nor is
/// `Unknown`, where the harness restarted mid-call and the call may well have
/// run.
pub fn never_ran(s: ResultStatus) -> bool {
    matches!(s, ResultStatus::Declined | ResultStatus::Cancelled)
}

/// How many distinct loop signatures one turn's counter holds. Past it the
/// counter stops learning new signatures and goes on counting the ones it has,
/// so a turn that asks for something different every loop costs bounded
/// memory and is never ended by this rule (its commonest signature stays at
/// one).
const MAX_SIGNATURES: usize = 512;

/// The loop signatures this turn has seen, and how often each one has.
#[derive(Debug, Default)]
pub struct Repeats {
    seen: BTreeMap<String, u32>,
    /// The highest count any signature has reached, kept so `most` is O(1)
    /// and so a signature dropped by the cap can never lower it.
    most: u32,
}

impl Repeats {
    /// Note the calls one loop asked for, and answer how often this loop's
    /// signature has now appeared in this turn, counting this one. The caller
    /// notes a loop only when `made_no_progress` says so; a loop with no tool
    /// calls counts nothing and answers 0, since it ends the turn by itself.
    pub fn note(&mut self, uses: &[ToolUse]) -> u32 {
        if uses.is_empty() {
            return 0;
        }
        let key = signature(uses);
        let room = self.seen.len() < MAX_SIGNATURES;
        let n = match self.seen.get_mut(&key) {
            Some(n) => {
                *n += 1;
                *n
            }
            // The cap: a signature the counter is not holding and has no room
            // for is counted as new every time, so it can never end a turn.
            None if room => {
                self.seen.insert(key, 1);
                1
            }
            None => 1,
        };
        self.most = self.most.max(n);
        n
    }

    /// Note this loop's calls only if the loop ran nothing
    /// (`made_no_progress`), which is the one case the repeat cap is for. The
    /// caller hands over one status per judged call and whether the turn is
    /// parked on a confirm; the rule it applies is the pure function below,
    /// and the turn loop stays two lines the lighter for it.
    pub fn note_idle(&mut self, uses: &[ToolUse], outcomes: &[ResultStatus], awaiting: bool) {
        if made_no_progress(outcomes, awaiting) {
            self.note(uses);
        }
    }

    /// The highest count any signature has reached in this turn.
    pub fn most(&self) -> u32 {
        self.most
    }
}

/// One loop's identity: each call's tool name and its arguments, in the order
/// the model asked for them. `serde_json`'s object map is a `BTreeMap` here
/// (no `preserve_order` feature in the workspace), so two inputs that differ
/// only in the order their keys arrived render the same bytes, and the
/// signature is canonical.
fn signature(uses: &[ToolUse]) -> String {
    let mut s = String::new();
    for u in uses {
        s.push_str(&u.name);
        s.push('\u{1f}');
        s.push_str(&u.input.to_string());
        s.push('\u{1e}');
    }
    s
}

/// What the owner is told when a turn ends on a repeat. `n` is how many times
/// the loop repeated, and `what` names the tools it asked for.
pub fn words(who: &str, n: u32, what: &str, stop_how: &str) -> String {
    format!(
        "{who} asked for the same work {n} times in this turn ({what}) and got no further, so the \
         turn ended rather than going on. Nothing was lost: the calls that ran, ran, and the ones \
         that were held were held. {OVERRIDE} To stop any work still running: {stop_how}."
    )
}

/// How the owner lifts the hold the repeat was spent against. The owner
/// decided on 2026-11-12 that every reversibility hold gains an override that
/// runs the ORIGINAL call as the model first proposed it, by a route that needs
/// him and that the model can never take, and that there is a chat route for it
/// as well as a command-line one. **The layer and the override now exist** —
/// `toolrun/reversible.rs` holds the call, `judge/undo.rs` constructs
/// `Input::Undo`, `toolrun::holds` keeps each held call as an offer, and
/// `reversible::allowed_original` spends an allowance the owner left for one
/// call — so the first two halves of the old note are no longer true and this
/// comment no longer says they are. What this sentence still says is only what
/// a turn ended by a repeat can promise: the next message is what decides, and
/// nothing is waiting on an answer. A row that wants to name the two routes
/// here replaces exactly this constant.
pub const OVERRIDE: &str =
    "If the work should go ahead as it was first proposed, say so in your next message: a repeat \
     ends the turn and asks nothing, so nothing is waiting on an answer.";

/// The tools a repeated loop asked for, worded for the notice: each distinct
/// tool name once, in order, at most four with the rest counted.
pub fn what(uses: &[ToolUse]) -> String {
    let mut names: Vec<&str> = Vec::new();
    for u in uses {
        if !names.contains(&u.name.as_str()) {
            names.push(&u.name);
        }
    }
    let more = names.len().saturating_sub(4);
    names.truncate(4);
    let head = names.join(", ");
    if more == 0 {
        head
    } else {
        format!("{head}, and {more} more")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn use_of(name: &str, args: serde_json::Value) -> ToolUse {
        ToolUse {
            id: "tu_0001".into(),
            name: name.into(),
            input: args,
        }
    }

    #[test]
    fn the_same_loop_asked_again_counts_up_and_a_different_one_does_not() {
        let mut r = Repeats::default();
        let held = vec![use_of("fs.write", json!({"path": "a.txt", "text": "x"}))];
        assert_eq!(r.note(&held), 1);
        assert_eq!(r.note(&held), 2);
        assert_eq!(r.note(&held), 3);
        assert_eq!(r.most(), 3);
        // Another plan is a signature of its own and starts at one.
        let other = vec![use_of("fs.write", json!({"path": "b.txt", "text": "x"}))];
        assert_eq!(r.note(&other), 1);
        // And the first one goes on from where it was.
        assert_eq!(r.note(&held), 4);
        assert_eq!(r.most(), 4);
    }

    #[test]
    fn a_fixing_loop_that_reruns_one_command_is_not_a_repeat() {
        // The counter-example the module doc names: identical `cargo test`
        // argv every loop, with a different edit beside it each time. Twelve
        // loops, and the commonest signature never passes one.
        let mut r = Repeats::default();
        let test = use_of("proc.run", json!({"argv": ["cargo", "test"]}));
        for i in 0..12 {
            let loops = vec![
                use_of("fs.edit", json!({"path": format!("src/f{i}.rs")})),
                test.clone(),
            ];
            assert_eq!(r.note(&loops), 1, "loop {i}");
        }
        assert_eq!(r.most(), 1);
        assert!(r.most() < MAX_REPEATS, "a fixing loop must never be ended");
        // The same command alone, with no edit between, IS the pathology.
        let alone = vec![test];
        for i in 1..=MAX_REPEATS {
            assert_eq!(r.note(&alone), i);
        }
        assert_eq!(r.most(), MAX_REPEATS);
    }

    #[test]
    fn only_a_loop_that_ran_nothing_counts_toward_the_cap() {
        use ResultStatus::*;
        // A loop whose calls ran made progress, whatever became of them. This
        // is the rule that keeps the detector off the product's three existing
        // instruments for a turn that gets nowhere.
        assert!(!made_no_progress(&[Ok], false));
        assert!(!made_no_progress(&[Background], false));
        assert!(!made_no_progress(&[Unknown], false));
        // `Error` means the tool RAN and failed, or its input was invalid, so
        // a command that fails six times running is not this rule's business.
        assert!(!made_no_progress(&[Error], false));
        assert!(!made_no_progress(&[Declined, Error], false));
        assert!(!made_no_progress(&[Error, Cancelled], false));
        // None of them ran: this is the loop the cap is for.
        assert!(made_no_progress(&[Declined], false));
        assert!(made_no_progress(&[Cancelled], false));
        assert!(made_no_progress(&[Declined, Cancelled], false));
        // A turn parked on a confirm is waiting for a person, not repeating.
        assert!(!made_no_progress(&[Declined], true));
        // Nothing judged is not a loop that ran nothing: a "no call ran" bar
        // must not be met by an empty set.
        assert!(!made_no_progress(&[], false));
        assert!(!made_no_progress(&[], true));
        // And the two halves of `never_ran`, named so a new status has to be
        // classified here rather than defaulting into the cap.
        assert!(never_ran(Declined) && never_ran(Cancelled));
        assert!(!never_ran(Ok) && !never_ran(Error) && !never_ran(Background));
        assert!(!never_ran(Unknown));
    }

    #[test]
    fn a_polling_loop_runs_its_wait_so_the_cap_never_sees_it() {
        // The hand-written exemption list this replaced was redundant AND
        // wrong: it held `job.wait` while a `ToolUse`'s name is the wire name
        // `job_wait`, so it matched nothing. The progress rule covers it
        // properly, because a wait runs and returns.
        assert_eq!(
            theseus_tools::wire_name(theseus_tools::jobs::WAIT),
            "job_wait"
        );
        assert!(!made_no_progress(&[ResultStatus::Ok], false));
        // The counter would have counted the loop, had it been asked to.
        let mut r = Repeats::default();
        let poll = vec![use_of("job_wait", json!({"job": "j1"}))];
        assert_eq!(r.note(&poll), 1);
        assert_eq!(r.note(&poll), 2);
    }

    #[test]
    fn a_loop_with_no_tool_calls_counts_nothing() {
        let mut r = Repeats::default();
        assert_eq!(r.note(&[]), 0);
        assert_eq!(r.note(&[]), 0);
        assert_eq!(r.most(), 0);
    }

    #[test]
    fn the_same_arguments_in_another_order_are_the_same_signature() {
        let mut r = Repeats::default();
        let a = vec![use_of("fs.write", json!({"path": "a", "text": "b"}))];
        let b = vec![use_of("fs.write", json!({"text": "b", "path": "a"}))];
        assert_eq!(r.note(&a), 1);
        assert_eq!(r.note(&b), 2, "the key order must not make a new signature");
        // The order of the CALLS is part of the identity, though.
        let mut r = Repeats::default();
        let one = use_of("fs.read", json!({"path": "a"}));
        let two = use_of("fs.read", json!({"path": "b"}));
        assert_eq!(r.note(&[one.clone(), two.clone()]), 1);
        assert_eq!(r.note(&[two, one]), 1);
    }

    #[test]
    fn a_turn_that_asks_something_new_every_loop_is_bounded_and_never_ended() {
        let mut r = Repeats::default();
        for i in 0..(MAX_SIGNATURES * 2) {
            let one = vec![use_of("fs.read", json!({"path": format!("f{i}")}))];
            assert_eq!(r.note(&one), 1, "loop {i}");
        }
        assert_eq!(r.most(), 1);
        assert_eq!(
            r.seen.len(),
            MAX_SIGNATURES,
            "the counter must stop learning at its cap"
        );
    }

    #[test]
    fn the_notice_names_the_count_the_tools_and_how_to_override() {
        let uses = vec![
            use_of("fs.write", json!({"path": "a"})),
            use_of("proc.run", json!({"argv": ["rm", "-rf", "x"]})),
            use_of("fs.write", json!({"path": "b"})),
        ];
        assert_eq!(what(&uses), "fs.write, proc.run");
        let text = words(
            "This session",
            6,
            &what(&uses),
            "`theseus stop -s ses_0001`",
        );
        assert!(text.contains("same work 6 times"), "{text}");
        assert!(text.contains("fs.write, proc.run"), "{text}");
        assert!(text.contains("theseus stop -s ses_0001"), "{text}");
        // The override sentence is in every notice: a turn ended by a repeat
        // that did not say how to go on is the failure this card guards.
        assert!(text.contains(OVERRIDE), "{text}");
        assert!(text.contains("say so in your next message"), "{text}");
        // Five distinct tools: four named and the rest counted.
        let many: Vec<ToolUse> = ["a.x", "b.x", "c.x", "d.x", "e.x"]
            .iter()
            .map(|n| use_of(n, json!({})))
            .collect();
        assert_eq!(what(&many), "a.x, b.x, c.x, d.x, and 1 more");
    }
}
