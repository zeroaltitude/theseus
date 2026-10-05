//! A refusal's client-side fallback (theseus-7gir.18; Eddie's decision of
//! 2026-10-04). When the model refuses a request and its catalog entry names a
//! fallback (`refusal_fallback_model`: Sonnet 5.5's is Sonnet 5), the same
//! request is made once more on that model, by the same provider, as Claude
//! Code does, and the rest of the turn runs there. A refusal there ends the
//! turn as any refusal does: never a chain, never a loop. Where the provider's
//! own fallback rides the request (`compiler::server_fallbacks`), it is used
//! instead, and `[model.retries] refusal = false` turns this one off.
//!
//! The compilation stays the profile model's. The requests name the fallback
//! (`RequestSpec::fallback`), leave the refused answer out with its "not run"
//! results, and carry both models' thinking back unchanged, as the provider
//! asks of a fallback: it drops what the other model cannot read. The session
//! keeps its own target, so its next turn starts on its own model.

use super::*;
use theseus_protocol::route::TurnFallback;

/// A turn's move to its model's fallback: what its result says, and where
/// the session ran before it, which its record keeps.
pub(super) struct FellBack {
    pub(super) fallback: TurnFallback,
    pub(super) ran_on: TargetRef,
}

impl TurnRunner {
    /// Whether the loop's refused answer goes to its model's fallback: the
    /// switch is on, the turn has not fallen back already, the provider's own
    /// fallback did not ride the request, and the catalog names one. Then the
    /// turn moves to it (its own reservation, its own price), the refused
    /// answer's text leaves the reply, and a `provider.fallback` row says so.
    pub(super) fn fall_back<'a>(
        &self,
        t: &mut Turn<'a>,
        spec: &mut RequestSpec,
        slot: &'a OnceLock<Target>,
        (resp, refused): (&ModelResponse, &Node),
        said_before: (usize, usize),
    ) -> bool {
        if resp.stop_reason.as_deref() != Some("refusal")
            || !self.cfg.model.retries.refusal
            || t.fallback.is_some()
        {
            return false;
        }
        let entry = self.catalog.get(&t.target.model);
        if crate::compiler::server_fallbacks(spec, entry) {
            return false;
        }
        let Some((to, cap)) = entry
            .and_then(|e| e.refusal_fallback_model.as_deref())
            .and_then(|to| Some((to, self.catalog.get(to)?.max_output_tokens)))
        else {
            return false;
        };
        let target = Target {
            model: to.to_string(),
            max_tokens: t.target.max_tokens.min(cap),
            ..t.target.clone()
        };
        if slot.set(target).is_err() {
            return false;
        }
        let category = resp
            .stop_details
            .as_ref()
            .and_then(|d| d["category"].as_str());
        let fallback = TurnFallback {
            from: t.target.model.clone(),
            to: to.to_string(),
            category: category.map(str::to_string),
            answered: false,
        };
        t.record(&fact::turn::FellBack {
            from: &fallback.from,
            to,
            category,
            loop_index: refused.loop_index,
            refused: &refused.id,
        });
        let ran_on = Self::ran_on(t);
        t.target = slot.get().expect("set");
        spec.fallback = Some((to.to_string(), refused.id.clone()));
        spec.max_tokens = t.target.max_tokens;
        t.output.truncate(said_before.0);
        t.said.truncate(said_before.1);
        t.fallback = Some(FellBack { fallback, ran_on });
        true
    }

    /// The fallback answered: a response of its that is not a refusal.
    pub(super) fn fallback_answered(t: &mut Turn<'_>, resp: &ModelResponse) {
        if let Some(f) = t.fallback.as_mut() {
            f.fallback.answered |= resp.stop_reason.as_deref() != Some("refusal");
        }
    }
}
