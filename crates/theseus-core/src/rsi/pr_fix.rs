//! The fix at the gate (issue 36; `join::fix`): a gate red in the change's own work is given once
//! to a fixer before the attempt ends. The fixer's commit is gated again by the same reading
//! (`judged`), and on green the builder's profile reads it alone; accepted, the attempt goes on
//! green, as any green attempt does. Anything else ends the attempt red, as today: one try, with
//! the report, the task's word and the model's result saying a fix was tried, and the item's next
//! attempt starting from before the fix (`fix::unfixed_base`). With `[self_fix] mode = "off"`, or a
//! red that is not the change's own (a keel stop, a red main, a fault of the machine, a check that
//! kept aborting, a gate that named no phase), nothing here runs.

use std::sync::Arc;

use serde_json::Value;

use super::tree::{GateRun, Place, Tree, Verdict};
use super::{judged, Attempt, Board};
use crate::rpc::Core;
use crate::rsi::join::fix::{self, make::Job, make::Made, Point};
use crate::toolrun::TurnCtx;

/// What a fix at the gate did, for the report, the task's word and the model's result.
#[derive(Debug, Clone)]
pub(super) struct Note {
    /// The phase the gate was red in.
    phase: String,
    /// What came of it, in a sentence.
    pub(super) words: String,
    made: Option<Made>,
    approver: Option<String>,
}

impl Note {
    /// The report's section.
    pub(super) fn section(&self) -> String {
        let made = self.made.as_ref().map_or_else(
            || "- Commit: none\n".to_string(),
            |m| {
                format!(
                    "- Fixer: {}\n- Commit: {} ({} lines in {} files), from {}\n",
                    m.fixer, m.head, m.lines, m.files, m.before
                )
            },
        );
        format!(
            "## The fix\n\n- Point: at the gate, for a red in {}\n{made}- Approved by: {}\n\
             - Outcome: {}\n\n",
            self.phase,
            self.approver.as_deref().unwrap_or("not approved"),
            self.words
        )
    }
}

/// The seams of a fix at the gate: the fixer and main's last gate.
#[derive(Clone)]
pub struct GateSeams {
    pub fixer: Arc<dyn fix::Fixer>,
    pub main: Arc<dyn crate::rsi::pipeline::main_red::Main>,
}

impl Default for GateSeams {
    fn default() -> Self {
        Self {
            fixer: Arc::new(fix::turns::Turns),
            // A test never reads the machine's own gate log.
            main: if cfg!(test) {
                Arc::new(crate::rsi::pipeline::main_red::Unrecorded)
            } else {
                Arc::new(crate::rsi::pipeline::main_red::Recorded::default())
            },
        }
    }
}

impl Board {
    /// The fix's seams: a test's, else the daemon's.
    pub(super) fn fix_seams(&self) -> GateSeams {
        self.fix
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_default()
    }

    /// A test's fixer and main.
    #[cfg(test)]
    pub fn set_fix(&self, s: GateSeams) {
        *self
            .fix
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(s);
    }
}

/// A branch that has its report is done: another attempt is a new branch, from before a fix of
/// it that did not hold.
pub(super) async fn next_attempt(core: &Core, place: &Place, mut t: Tree) -> Result<Tree, String> {
    let p = place.clone();
    let from = fix::unfixed_base(&core.store, &t.branch);
    super::blocking(move || {
        super::tree::next_branch(&p, &mut t)?;
        if let Some(from) = from {
            super::tree::reset_to(&p, &t, &from)?;
        }
        Ok(t)
    })
    .await
}

/// The gate's reading of the tree (`judged`), and a red given once to a fixer when it may be: the
/// run the attempt goes on with and the branch's commits, or the way out of a halt.
pub(super) async fn gated(
    core: &Arc<Core>,
    board: &Board,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
    commits: Vec<(String, String)>,
) -> Result<(GateRun, Vec<(String, String)>), Result<(String, Value), String>> {
    let run = judged(a, place).await?;
    let run = after_red(core, board, place, tc, a, run).await?;
    if a.fix.is_none() {
        return Ok((run, commits));
    }
    let (p, t) = (place.clone(), a.tree.clone());
    match super::blocking(move || super::tree::commits(&p, &t)).await {
        Ok(c) => Ok((run, c)),
        Err(e) => Err(Err(e)),
    }
}

/// A red gate, once to a fixer when it may be: the run the attempt goes on with (green when the
/// fix held), or the way out of a halt.
async fn after_red(
    core: &Arc<Core>,
    board: &Board,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
    run: GateRun,
) -> Result<GateRun, Result<(String, Value), String>> {
    if run.verdict != Verdict::Red || !core.cfg.self_fix.on() || super::stopped(core).is_some() {
        return Ok(run);
    }
    let Ok(phase) = fix::own_work(&run) else {
        return Ok(run);
    };
    let seams = board.fix_seams();
    if seams.main.red(place).is_some() {
        return Ok(run);
    }
    let Ok(main) = crate::rsi::join::git::main_of(place, "origin") else {
        return Ok(run);
    };
    let (tree, item, why) = (a.tree.clone(), a.i.item.trim().to_string(), a.i.why.clone());
    let job = Job {
        core,
        place,
        tree: &tree,
        item: &item,
        point: Point::Gate,
        builder: Some(tc.session_id),
        why: &why,
        fault: phase.clone(),
        failure: run.tail.clone(),
        main,
        fixer: seams.fixer,
    };
    let note = |words: String, made: Option<Made>, approver: Option<String>| Note {
        phase: phase.clone(),
        words,
        made,
        approver,
    };
    let made = match job.make().await {
        Ok(m) => m,
        Err(why) => {
            a.fix = Some(note(why, None, None));
            return Ok(run);
        }
    };
    a.head = Some(made.head.clone());
    let (p, t) = (place.clone(), a.tree.clone());
    if let Ok(d) = super::blocking(move || super::tree::diffstat(&p, &t)).await {
        a.diff = d;
    }
    let again = judged(a, place).await?;
    if again.verdict != Verdict::Green {
        let why = job.red_again(&made, &again.tail);
        a.fix = Some(note(why, Some(made), None));
        return Ok(again);
    }
    match job.approve(&made).await {
        Ok(approver) => {
            let words = format!(
                "the gate's red in {phase} was repaired by {}, the gate is green again, and {approver} \
                 accepted the fix",
                made.fixer
            );
            a.fix = Some(note(words, Some(made), Some(approver)));
            Ok(again)
        }
        Err(why) => {
            let mut red = again;
            red.verdict = Verdict::Red;
            red.tail = format!(
                "fix: the gate was green on the fix, but {why}\n{}",
                red.tail
            );
            a.fix = Some(note(why, Some(made), None));
            Ok(red)
        }
    }
}
