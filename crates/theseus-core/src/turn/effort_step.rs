//! A model that refuses per-message effort, found out at run time
//! (theseus-posww). `compiler::effort::default_of` names only the model seen
//! to accept the effort-only system messages, but a provider can change what
//! it accepts: on 2026-10-11 Sonnet 5.5 answered one with a 400 (`messages.190.
//! output_config: Extra inputs are not permitted`), and every request of a
//! conversation that held one failed. A 400 that names `output_config` on a
//! request that carries such a message is asked again once, in the same turn,
//! with those messages left out and the turn's level at the top; the model is
//! marked for the daemon's life, so no later request carries one, and a
//! `provider.effort_refused` row says so. A restart forgets the mark: the
//! first request after it learns it again, at the price of one failed call.

use std::collections::BTreeSet;
use std::sync::Mutex;

use super::*;

/// The models this daemon found to refuse per-message effort.
#[derive(Default)]
pub struct EffortRefusals(Mutex<BTreeSet<String>>);

impl EffortRefusals {
    fn mark(&self, model: &str) -> bool {
        self.0.lock().unwrap().insert(model.to_string())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().cloned().collect()
    }
}

impl TurnRunner {
    /// The models a request spec says take no per-message effort.
    pub(super) fn effort_refused(&self) -> Vec<String> {
        self.effort_refusals.all()
    }

    /// Whether the loop asks again without per-message effort: `failure` is
    /// the provider's 400 on the messages, and the turn has not asked again
    /// for it already (`asked`). Then the spec leaves them out for the rest of
    /// the turn, the turn's own level moves to the top-level effort, the
    /// model is marked, and the row says so.
    pub(super) fn without_message_effort(
        &self,
        t: &mut Turn<'_>,
        spec: &mut RequestSpec,
        compiled: &Compiled,
        failure: &Failure,
        asked: &mut bool,
    ) -> bool {
        let error = format!("{:#}", failure.source);
        if *asked || !crate::compiler::effort::refused_by(&compiled.request, &failure.class, &error)
        {
            return false;
        }
        *asked = true;
        let model = compiled.request.model.clone();
        self.effort_refusals.mark(&model);
        if !spec.effort_refused.contains(&model) {
            spec.effort_refused.push(model.clone());
        }
        // The level the turn ran at moves to the top, where this model reads it.
        if let Some(e) = spec.turn_effort.take() {
            spec.effort = Some(e);
        }
        t.record(&fact::turn::EffortRefused {
            model: &model,
            error: &failure.reason,
        });
        t.record(&fact::turn::LoopCut {
            decision: "effort_refused",
        });
        true
    }
}
