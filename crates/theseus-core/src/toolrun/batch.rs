//! A batch's gate (`proc.run`'s `steps`, theseus-7gir.3): each step is
//! judged as the call it would be alone, through the whole order (the
//! place's refusal, the floor, the lists, a grant's posture), and
//! the batch takes the strictest. A refusal of any step refuses it; else the
//! floor over approve over notify over open, its reason naming the step
//! that set it. The proposal a confirmation binds is the whole call's input,
//! so one approval binds every step, and a batch that differs asks again.
//! A batch runs every step in one class, the one its proposal binds, so
//! steps that differ in class are invalid input (theseus-nrvq): an L0
//! step's approval would otherwise run an `l1_argv` program at L0. `gate`
//! runs it, and so does the test that holds `policy.explain` to it.

use serde_json::Value;
use theseus_tools::{Plan, Tool};

use super::order::{At, Layer};
use super::ToolRuntime;
use crate::policy::Decision;
use crate::sandbox::Bound;

/// Why `judge` turns a planned call away.
#[derive(Debug)]
pub(crate) enum Turned {
    /// The place refuses it: the place rule's word, or the ceiling's.
    Place(String),
    /// Its input is invalid: a batch whose steps differ in class.
    Invalid(String),
}

/// One thing the order judges: a plain call, or one step of a batch, as the
/// refusals admitted it.
#[derive(Debug)]
pub(crate) struct Judged {
    /// The step as a batch's reason names it (`step 2 of 3 (`rm -rf x`)`).
    pub named: String,
    pub plan: Plan,
    pub input: Value,
}

/// What the refusals admitted: a plain call's one plan, or a batch's steps.
/// A batch is told apart from a one-step call because its reason names the
/// step and a plain call's does not.
#[derive(Debug)]
pub(crate) enum Admits {
    /// Boxed: a `Judged` holds a whole `Plan`, and the two variants differ
    /// by some hundreds of bytes without it.
    One(Box<Judged>),
    Batch(Vec<Judged>),
}

impl ToolRuntime {
    /// The place's refusal, then the order, for a planned call: for a batch,
    /// every step's, and the strictest. Err: why the place refuses it, or
    /// why its input is invalid.
    ///
    /// The two halves are apart because the reversibility layer (0d) sits
    /// between them: a call the gate would refuse outright must not have its
    /// content sent to a model first (`docs/design/undo-v1.md`, "0d sits
    /// AFTER the outright refusals"). Every caller that asks nothing — the
    /// explain probe, the tests, a daemon with the pack off — gets exactly
    /// what it got before, because this runs both in order. The gate's own
    /// path goes through `judge_reversibility`, which is this with 0d
    /// between the halves, so this composition is now the tests' alone.
    #[cfg(test)]
    pub(crate) fn judge(
        &self,
        at: &At<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
        seen: &mut dyn FnMut(Layer, &Decision),
    ) -> Result<(Decision, Bound), Turned> {
        let admits = self.refusals(at, tool, plan, input)?;
        self.ordered(at, tool, &admits, seen)
    }

    /// The outright refusals, and nothing else: the sealed holdout on the
    /// call's raw input, then, for a call and for every step of a batch, the
    /// place rule, the place's ceiling, plan mode, a helper's set and the
    /// injection freeze (`ToolRuntime::refusal_at`). Nothing here reads a
    /// model, so a refused call's content never leaves the daemon.
    pub(crate) fn refusals(
        &self,
        at: &At<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
    ) -> Result<Admits, Turned> {
        if let Some(why) = self.sealed.refusal_of_input(input) {
            return Err(Turned::Place(why));
        }
        let Some(steps) = tool.steps(input) else {
            if let Some(why) = self.refusal_at(at, tool, plan) {
                return Err(Turned::Place(why));
            }
            return Ok(Admits::One(Box::new(Judged {
                named: String::new(),
                plan: plan.clone(),
                input: input.clone(),
            })));
        };
        let n = steps.len();
        let mut out = Vec::with_capacity(n);
        for (i, one) in steps.iter().enumerate() {
            let p = tool
                .plan(one, at.ctx)
                .map_err(|e| Turned::Invalid(format!("step {} of {n}: {e}", i + 1)))?;
            let argv = p.argv.as_deref().unwrap_or_default();
            let named = format!("step {} of {n} (`{}`)", i + 1, argv.join(" "));
            if let Some(why) = self.refusal_at(at, tool, &p) {
                return Err(Turned::Place(format!("{named}: {why}")));
            }
            out.push(Judged {
                named,
                plan: p,
                input: one.clone(),
            });
        }
        Ok(Admits::Batch(out))
    }

    /// The whole §3.9 order over what the refusals admitted, and for a batch
    /// the strictest step's, its reason naming that step.
    pub(crate) fn ordered(
        &self,
        at: &At<'_>,
        tool: &dyn Tool,
        admits: &Admits,
        seen: &mut dyn FnMut(Layer, &Decision),
    ) -> Result<(Decision, Bound), Turned> {
        let steps = match admits {
            Admits::One(j) => return Ok(self.order(at, tool, &j.plan, &j.input, seen)),
            Admits::Batch(steps) => steps,
        };
        let mut worst: Option<(String, Decision, Bound)> = None;
        let mut granted = Vec::new();
        let mut classes = Vec::new();
        for j in steps {
            let argv = j.plan.argv.as_deref().unwrap_or_default();
            let (d, b) = self.order(at, tool, &j.plan, &j.input, seen);
            // What chose L1, as `sandbox::unbrokered` asked it.
            let l1 = b
                .l1()
                .then(|| self.sandbox.l1_for(argv, &j.input).unwrap_or_default());
            classes.push((j.named.clone(), l1));
            if let Some(g) = &d.granted {
                if !granted.contains(g) {
                    granted.push(g.clone());
                }
            }
            if worst.as_ref().is_none_or(|(_, w, _)| stricter(&d, w)) {
                worst = Some((j.named.clone(), d, b));
            }
        }
        one_class(&classes)?;
        let (named, mut d, b) =
            worst.ok_or_else(|| Turned::Invalid("steps must hold at least one step".into()))?;
        d.reason = format!("{named}: {}", d.reason);
        if let Some(n) = d.notify.as_mut() {
            n.rule = d.reason.clone();
        }
        d.granted = (!granted.is_empty()).then(|| granted.join("; "));
        Ok((d, b))
    }
}

/// The floor over approve over notify over open: a tie keeps the earlier
/// step.
fn stricter(a: &Decision, b: &Decision) -> bool {
    (a.floor, a.posture) > (b.floor, b.posture)
}

/// One class for every step (theseus-nrvq, option 1): `run_steps` launches
/// each in the class the batch binds, so a stricter L0 step would take an
/// `l1_argv` step out of L1, and nothing falls back from L1 to L0. Err
/// names each step's class, and what chose L1.
fn one_class(steps: &[(String, Option<String>)]) -> Result<(), Turned> {
    if steps
        .iter()
        .all(|(_, l1)| l1.is_some() == steps[0].1.is_some())
    {
        return Ok(());
    }
    let mut each: Vec<String> = steps
        .iter()
        .enumerate()
        .map(|(i, (named, l1))| {
            let runs = if i == 0 { "runs " } else { "" };
            match l1 {
                Some(why) => format!("{named} {runs}in L1 ({why})"),
                None => format!("{named} {runs}at L0"),
            }
        })
        .collect();
    let last = each.pop().unwrap_or_default();
    Err(Turned::Invalid(format!(
        "{} and {last}: a batch runs in one class; split it",
        each.join(", ")
    )))
}
