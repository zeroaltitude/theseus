//! The reversibility layer (theseus-p5kln, row B6 `theseus-2yowd`): 0d, and
//! the two places its answer is read.
//!
//! The design is `docs/design/undo-v1.md` and the question is
//! `crates/theseus-judge/packs/undo.v1.toml`; this is the code they describe.
//! **ASK ONCE, CONSUME TWICE, ON OPPOSITE SIDES OF THE ORDER:**
//!
//! ```text
//! ToolRuntime::judge_reversibility
//!   |
//!   |-- refusals(...)     the sealed holdout, the place rule, the place's
//!   |                     ceiling, plan mode, the helper's set, the
//!   |                     injection freeze. Nothing is sent to a model
//!   |                     before these: a call the gate was going to refuse
//!   |                     must not have its content, its paths and the bytes
//!   |                     it was about to write serialised and sent first.
//!   |
//!   |-- 0d: ask undo.v1, ONCE, for this call
//!   |
//!   |-- POINT ONE: `irreversible_reshapeable` short-circuits HERE, beside
//!   |              the refusals. `order` never runs, so no card is made and
//!   |              nothing can lower a hold: a hold is not a posture.
//!   |
//!   |-- ordered(...)      the whole §3.9 order: Policy, Mode, Grant, Lsp,
//!   |                     SharedFetch, Floor, Glide, Routine, McpClient
//!   |
//!   `-- POINT TWO: `irreversible_final` raises what `order` RETURNED. Every
//!                  layer has had its say, so `Layer::Mode` cannot lower it
//!                  in an `edits` or `auto` session and no `floor: true` has
//!                  to be invented to protect it.
//!
//! **WHY POINT TWO IS AFTER AND NOT INSIDE THE LADDER.** `policy/modes.rs`'s
//! `gate` exists to LOWER an `Approve` in an `edits` or `auto` session, in a
//! private place, on an untightened tool with `floor` unset — which is
//! exactly the shape of this raise. A raise placed before it would read as
//! present in the code and be absent in the product in `auto` mode, which is
//! the mode a long coding session runs in. The order's own invariant (each
//! layer after the first only raises) is what makes the late raise safe: it
//! can only leave an already-stricter decision alone.
//!
//! **FAIL OPEN.** No verdict means the ordinary order, for every reason there
//! can be none: the judge off, the pack in shadow, the credential
//! unresolved, the breaker open, the call shed, the 300 ms wait missed, or an
//! answer under the question's confirm bar. The reason is the pack's: this
//! classifier is a net addition to a gate that still has every guard it had
//! in 1.0.4, so failing open returns the product to the release before it,
//! while failing closed would make Jev a hard dependency of writing a file.

use serde_json::Value;
use theseus_tools::{Backend, Plan, Tool};

use super::batch::Turned;
use super::holds;
use super::order::{At, Layer};
use super::{ToolRuntime, TurnCtx};
use crate::judge::undo::{self, UndoVerdict};
use crate::policy::{Decision, Posture};
use crate::sandbox::Bound;

/// The `reason` a held call's record and row carry, as `place:` and `mode:`
/// name the other two kinds of "this call did not run".
pub const HELD_REASON: &str = "undo";

/// What the gate came to for one planned call, once the refusals, 0d, the
/// order and a glide's own word have all had their say. It is its own type
/// so `gate` stays inside the shape budget: the three ways a call does not
/// run differ only in the words and the record each leaves.
#[derive(Debug)]
pub(crate) enum Decided {
    Runs(Decision, Bound),
    /// It will not run: the words the model reads, whether that is a
    /// refusal (as against invalid input), and the hold where there is one.
    Stopped {
        why: String,
        refused: bool,
        held: Option<Box<Hold>>,
    },
}

/// Why the gate turned a planned call away, with the hold beside the two
/// answers that existed before. `Turned` itself is untouched: it is matched
/// in files this row may not edit, and a third variant there would have
/// meant editing them (`Turned` is `batch.rs`'s).
#[derive(Debug)]
pub(crate) enum Blocked {
    /// The refusals' or the order's own word.
    Turned(Turned),
    /// `undo.v1` judged that the call would lose something a reshaped call
    /// would keep: it is held, the model re-plans it, and no card is made.
    /// `order` never ran for it.
    Held(Hold),
}

/// What a spend of the owner's allowance came to, and so whether the call
/// runs. Three outcomes where the code had one answer: before 2026-11-13 the
/// two that are not `Recorded` shared an arm whose comment said "the call
/// runs either way".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spend {
    /// The allowance was this call's and this frame recorded it spent.
    Recorded,
    /// The allowance was read as waiting and the spend's own frame found it
    /// already spent.
    AlreadySpent,
    /// The spend's frame was refused, so no record says this allowance was
    /// consumed.
    Unwritten,
}

impl Spend {
    /// Whether the call runs. **Only a spend this frame RECORDED admits it.**
    ///
    /// `AlreadySpent` is the owner's word of 2026-11-13 17:48 (S34-2): one
    /// allowance admits one call, so a call that finds its allowance already
    /// spent is held again, as any other reversibility hold.
    ///
    /// `Unwritten` goes the same way, and the argument is
    /// [`ToolRuntime::spend_allowance`]'s own: the spend lands BEFORE the
    /// call precisely so that nothing between them can let one allowance run
    /// the call twice. That ordering already declares which cost is the
    /// cheaper one -- an allowance spent with nothing run "costs the owner
    /// one more word", and the order it rejects is the one that could run a
    /// call twice. A refused frame leaves exactly the state that ordering
    /// exists to forbid: the allowance still reads as waiting, so a later
    /// call of this session with the same bytes would find it and run too.
    /// Running on it would reach that state through a bad disk instead of
    /// through a crash, which is no better.
    ///
    /// It is also the frame that carries the `tool.hold_overridden` row. A
    /// store that cannot take it cannot record that an irreversible call was
    /// ever authorised, and running the one class of call the gate judged
    /// unrecoverable with no record of its authority is the worse of the two
    /// failures by a distance.
    ///
    /// The module's FAIL OPEN doctrine does not reach here: it is about a
    /// missing VERDICT, where the ordinary order returns the product to
    /// 1.0.4. There is a verdict here and it holds. The owner's remedy costs
    /// him one word and is already built -- the call is offered for an
    /// override again.
    pub(crate) fn admits(self) -> bool {
        matches!(self, Spend::Recorded)
    }
}

/// A call held at point one: what the model reads, and what the record keeps
/// so the owner can still say the original was right.
#[derive(Debug, Clone)]
pub(crate) struct Hold {
    /// The sentence the model reads, built from the pack's own words.
    pub why: String,
    /// The judgment that held it.
    pub judgment: String,
    /// The shape the pack chose, where it chose one above its own bar.
    pub shape: Option<String>,
    /// The call in one line, as the plan summarised it: what the owner reads
    /// in `theseus undo list` (theseus-p5kln).
    pub summary: String,
}

impl ToolRuntime {
    /// One planned call through the refusals, 0d, the order, the thread
    /// guard's doubt and a glide's own word, in that order: everything the
    /// gate decides about a call it was able to plan.
    pub(crate) async fn decided(
        &self,
        tc: &TurnCtx<'_>,
        at: &At<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
        glided: Option<String>,
    ) -> Decided {
        let stop = |why: String, refused: bool| Decided::Stopped {
            why,
            refused,
            held: None,
        };
        match self
            .judge_reversibility(tc, at, tool, plan, input, &mut |_, _| {})
            .await
        {
            // A batch whose steps differ in class (theseus-nrvq).
            Err(Blocked::Turned(Turned::Invalid(e))) => stop(e, false),
            Err(Blocked::Turned(Turned::Place(why))) => stop(why, true),
            Err(Blocked::Held(h)) => Decided::Stopped {
                why: h.why.clone(),
                refused: true,
                held: Some(Box::new(h)),
            },
            Ok((d, class)) => match glided {
                Some(why) => stop(why, true),
                None => Decided::Runs(self.misroute_hold(tc, tool, plan, d), class),
            },
        }
    }

    /// The refusals, 0d, and the order, with the verdict read on both sides
    /// of it. `Err(Blocked::Held)`: point one held the call.
    pub(crate) async fn judge_reversibility(
        &self,
        tc: &TurnCtx<'_>,
        at: &At<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
        seen: &mut (dyn FnMut(Layer, &Decision) + Send),
    ) -> Result<(Decision, Bound), Blocked> {
        let admits = self
            .refusals(at, tool, plan, input)
            .map_err(Blocked::Turned)?;
        // 0d. After every outright refusal, before the order.
        let verdict = self.undo_check(tc, tool, plan, input).await;
        // Point one, and the owner's override of it (theseus-p5kln). A hold
        // nobody overrode is the hold it always was: the call does not run,
        // the model re-plans it, and no card is made. An override admits
        // THIS call and only this one -- `allowance_for` matches the tool
        // and the input the owner was shown -- and it admits it into the
        // ORDINARY ORDER, which keeps every say it had: the override is of
        // the hold, not of the gate.
        //
        // ONLY A SPEND THIS FRAME RECORDED ADMITS THE CALL (the owner,
        // 2026-11-13 17:48, S34-2). An allowance found already spent is a
        // hold again, and so is one whose spend could not be written:
        // [`Spend::admits`] has the argument. The hold is the ordinary one
        // -- the same status, counted by the repeat cap, and offered for an
        // override again -- because it is built by the same `hold_of` the
        // no-allowance arm builds.
        if let Some(v) = verdict.as_ref().filter(|v| v.holds()) {
            let admits = match Self::allowed_original(tc, tool, input) {
                None => false,
                Some(id) => Self::spend_allowance(tc, &id, tool, plan, v).admits(),
            };
            if !admits {
                return Err(Blocked::Held(self.hold_of(tc, tool, plan, v)));
            }
        }
        let (decision, bound) = self
            .ordered(at, tool, &admits, seen)
            .map_err(Blocked::Turned)?;
        // Point two, on what the order returned.
        let decision = match verdict.as_ref().filter(|v| v.raises()) {
            None => decision,
            Some(v) => {
                let d = raised(decision, tool.name(), &plan.summary);
                seen(Layer::Reversibility, &d);
                tc.record(&crate::fact::tool::ToolRaised {
                    tool: tool.name(),
                    judgment: &v.judgment,
                    p: v.p,
                    summary: &plan.summary,
                });
                d
            }
        };
        Ok((decision, bound))
    }

    /// 0d itself: `undo.v1` asked once about this planned call. `None` is
    /// every fail-open arm, and it is also what a call the pack can say
    /// nothing useful about comes to, which is asked about and billed for
    /// nowhere ([`UndoCall::worth_asking`](crate::judge::undo::state::UndoCall::worth_asking)).
    async fn undo_check(
        &self,
        tc: &TurnCtx<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
    ) -> Option<UndoVerdict> {
        let judge = self.judge.get()?;
        if !judge.undo_on(tc.session_id) {
            return None;
        }
        let is_program = tool.backend() == Backend::Job;
        let l1 = is_program
            && self
                .sandbox
                .l1_for(plan.argv.as_deref().unwrap_or_default(), input)
                .is_some();
        // What this session's own record holds (`p5kln-location-facts`): the
        // read board, asked per target. `holds` is one `stat` and a compare,
        // never a transcript scan, so it belongs on this path.
        let holds = |p: &std::path::Path| self.reads.holds(tc.session_id, p);
        let call = undo::state::UndoCall::of(
            tc.session_id,
            tc.turn_id,
            tool,
            plan,
            input,
            undo::state::Knows {
                is_program,
                l1,
                holds: &holds,
            },
        );
        if !call.worth_asking() {
            return None;
        }
        judge.judge_undo(call).await
    }

    /// The allowance the owner left for THIS call, if there is one: the id
    /// of a hold of this session whose tool and input are the call's own
    /// bytes, allowed and not yet spent (`toolrun::holds`). A keyed META
    /// read, through `theseus_store::blocking` because the gate's path is a
    /// runtime worker's, and reached only when a hold would otherwise fire --
    /// so a call nothing holds pays nothing for the route.
    fn allowed_original(tc: &TurnCtx<'_>, tool: &dyn Tool, input: &Value) -> Option<String> {
        let store = tc.store;
        let (session, name) = (tc.session_id, tool.name());
        theseus_store::blocking(|| match holds::Holds::read(store) {
            Ok(h) => h.allowance_for(session, name, input),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the held calls could not be read");
                None
            }
        })
    }

    /// Spend the allowance, in a frame of its own, BEFORE the call runs: a
    /// crash between the spend and the call leaves the allowance spent and
    /// nothing run, which costs the owner one more word; the other order
    /// would let one allowance run the call twice. One frame holds the
    /// holds' record and the `tool.hold_overridden` row.
    ///
    /// What it came to is the caller's to read: the three outcomes are three
    /// different things and only one of them admits the call ([`Spend`]).
    fn spend_allowance(
        tc: &TurnCtx<'_>,
        id: &str,
        tool: &dyn Tool,
        plan: &Plan,
        v: &UndoVerdict,
    ) -> Spend {
        let (store, rec) = (tc.store, tc.rec());
        let fact = crate::fact::tool::ToolHoldOverridden {
            hold: id,
            tool: tool.name(),
            judgment: &v.judgment,
            shape: v.shape.as_deref(),
            summary: &plan.summary,
        };
        let written = theseus_store::blocking(|| -> anyhow::Result<bool> {
            let mut held = holds::Holds::read(store)?;
            if !held.spend(id, theseus_protocol::now_unix_ms()) {
                return Ok(false);
            }
            store.append(&[rec.row(&fact)?, held.record()?])?;
            Ok(true)
        });
        // The three outcomes say three different things, and an operator
        // reading a trace must be able to tell which happened: the message
        // and the level differ, and each names what became of the call.
        match written {
            Ok(true) => {
                rec.announce(&fact);
                Spend::Recorded
            }
            // The allowance was read as waiting and the spend's own frame
            // found it already spent. One allowance admits ONE call.
            Ok(false) => {
                tracing::info!(hold = %id,
                    "the allowance was already spent: the call is held again");
                Spend::AlreadySpent
            }
            // The store refused the frame, so nothing records that this
            // allowance was consumed.
            Err(e) => {
                tracing::warn!(hold = %id, error = %format!("{e:#}"),
                    "the allowance could not be recorded as spent: the call is held again");
                Spend::Unwritten
            }
        }
    }

    /// A hold's words and its record. The shape's sentence is the pack's
    /// own, read from the pack that answered, so the six shapes the owner
    /// can argue with are the six the model is told about.
    fn hold_of(&self, tc: &TurnCtx<'_>, tool: &dyn Tool, plan: &Plan, v: &UndoVerdict) -> Hold {
        let means = self
            .judge
            .get()
            .and_then(|j| j.pack(undo::PACK_NAME))
            .and_then(|p| v.shape_means(&p));
        let why = words(&plan.summary, means.as_deref());
        tc.record(&crate::fact::tool::ToolHeld {
            tool: tool.name(),
            judgment: &v.judgment,
            shape: v.shape.as_deref(),
            p: v.p,
            summary: &plan.summary,
        });
        Hold {
            why,
            judgment: v.judgment.clone(),
            shape: v.shape.clone(),
            summary: plan.summary.clone(),
        }
    }
}

/// What the model reads when a call is held. Facts and one instruction: what
/// was held, that something would be lost, and the shape to take instead.
/// It does not invite the same call again — a hold the model answers by
/// re-proposing what it proposed is the loop the repeat cap exists to end
/// (`p5kln-p0-unbounded-hold`).
pub(crate) fn words(summary: &str, means: Option<&str>) -> String {
    let mut s = format!(
        "{summary}: this would lose something that nothing holds a copy of, \
         and the same work has a shape that keeps a way back."
    );
    match means {
        Some(m) => s.push_str(&format!(" Do it this way instead: {m}")),
        None => s.push_str(
            " Do it a way that keeps a way back — move the targets aside, \
             write beside the file and rename into place, copy or export \
             first, publish under a new name, or name the exact targets \
             instead of a sweep — and say which you chose.",
        ),
    }
    s
}

/// Point two: the decision the order returned, raised to `approve`. The
/// `why` is the clause every surface shows the owner
/// (`needs_you::rule_clause` reads the last `— approve (…)`).
fn raised(d: Decision, tool: &str, summary: &str) -> Decision {
    d.at_least(
        Posture::Approve,
        "undo.v1: nothing would bring back what this loses",
        "[judge] reversibility",
        tool,
        summary,
    )
}

/// The `reason` a held call's gate record and result node carry, beside
/// `place: …` and `mode: …`. The shape is in it, so the ledger row and the
/// stored record both say which of the six was suggested.
pub(crate) fn reason(h: &Hold) -> String {
    match &h.shape {
        Some(shape) => format!("{HELD_REASON}: {shape} ({})", h.judgment),
        None => format!("{HELD_REASON}: no shape named ({})", h.judgment),
    }
}
