//! A batch's gate (`proc.run`'s `steps`, theseus-7gir.3): each step is
//! judged as the call it would be alone, through the whole order (the
//! place's refusal, the floor, the lists, a grant's posture, the hold), and
//! the batch takes the strictest. A refusal of any step refuses it; else the
//! floor over approve over notify over open, its reason naming the step
//! that set it. The proposal a confirmation binds is the whole call's input,
//! so one approval binds every step, and a batch that differs asks again.
//! `gate` runs it, and so does the test that holds `policy.explain` to it.

use serde_json::Value;
use theseus_tools::{Plan, Tool};

use super::order::{At, Layer};
use super::ToolRuntime;
use crate::policy::Decision;
use crate::sandbox::Bound;

impl ToolRuntime {
    /// The place's refusal, then the order, for a planned call: for a batch,
    /// every step's, and the strictest. Err: why the place refuses it.
    pub(crate) fn judge(
        &self,
        at: &At<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
        seen: &mut dyn FnMut(Layer, &Decision),
    ) -> Result<(Decision, Bound), String> {
        let Some(steps) = tool.steps(input) else {
            if let Some(why) = self.refusal(at.place, tool.name(), plan) {
                return Err(why);
            }
            return Ok(self.order(at, tool, plan, input, seen));
        };
        let n = steps.len();
        let mut worst: Option<(String, Decision, Bound)> = None;
        let mut granted = Vec::new();
        for (i, one) in steps.iter().enumerate() {
            let p = tool
                .plan(one, &self.ctx)
                .map_err(|e| format!("step {} of {n}: {e}", i + 1))?;
            let named = format!(
                "step {} of {n} (`{}`)",
                i + 1,
                p.argv.as_deref().unwrap_or_default().join(" ")
            );
            if let Some(why) = self.refusal(at.place, tool.name(), &p) {
                return Err(format!("{named}: {why}"));
            }
            let (d, b) = self.order(at, tool, &p, one, seen);
            if let Some(g) = &d.granted {
                if !granted.contains(g) {
                    granted.push(g.clone());
                }
            }
            if worst.as_ref().is_none_or(|(_, w, _)| stricter(&d, w)) {
                worst = Some((named, d, b));
            }
        }
        let (named, mut d, b) = worst.ok_or("steps must hold at least one step")?;
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
