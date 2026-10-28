//! A refusal's client-side fallback (theseus-7gir.18; the owner's decision of
//! 2026-10-04). When the model refuses a request and its catalog entry names a
//! fallback (`refusal_fallback_model`: Sonnet 5.5's is Sonnet 5), the same
//! request is made once more on that model, by the same provider, as Claude
//! Code does. The fallback answers that request and the next goes back to the
//! routed model (theseus-wf482): a refusal is usually incidental, and the
//! rest of the turn belongs to the model the turn was routed to. A refusal
//! there ends the turn as any refusal does: never a chain, never a loop.
//! Where the provider's own fallback rides the request
//! (`compiler::server_fallbacks`), it is used instead, and `[model.retries]
//! refusal = false` turns this one off.
//!
//! Before it, a refusal whose category is not a genuine decline is made once
//! more on the routed model (theseus-keuap, `[model.retries] refusal_retry`):
//! the request is the same, the refused answer left out as the fallback's
//! leaves it out. It answers, and the turn goes on as if the refusal had not
//! been; it refuses, and the fallback takes the request as above. A retry is
//! one more provider call, reserved, settled and ledgered as any, and it is
//! not a routed refusal toward `STAYS_AT`: that counts the refusals the
//! fallback had to answer. At most one retry per refused request and
//! `RETRIES_PER_TURN` per turn; a hard decline is never retried.
//!
//! The turn stays on the fallback (`latched`) when the refusal's category is
//! one of `[model.retries] hard_decline_categories`, when a call was cut at its
//! whole-call bound (the same request would run as long again), or at the
//! third routed refusal in a row: a genuine decline does not bounce back to the
//! model that declined it.
//!
//! The compilation stays the profile model's. The requests name the fallback
//! (`RequestSpec::fallback`), leave the refused answer out with its "not run"
//! results, and carry both models' thinking back unchanged, as the provider
//! asks of a fallback: it drops what the other model cannot read. The request
//! that goes back to the routed model carries the routed model's thinking
//! alone. The session keeps its own target, so its next turn starts on its
//! own model.

use super::*;
use theseus_protocol::route::TurnFallback;

/// The routed refusals in a row, counted from the routed model's last answer,
/// at which a turn stays on the fallback.
const STAYS_AT: u32 = 3;

/// The same-model retries a turn makes, in all.
const RETRIES_PER_TURN: u32 = 3;

/// A turn's same-model retries of refusals.
#[derive(Default)]
pub(super) struct Again {
    /// Retries made so far.
    used: u32,
    /// The request in flight is a retry: its refusal goes to the fallback.
    pending: bool,
}

/// A turn's moves to its model's fallback: what its result says, where the
/// session ran before the first, which its record keeps, and the target the
/// turn goes back to.
pub(super) struct FellBack<'a> {
    pub(super) fallback: TurnFallback,
    pub(super) ran_on: TargetRef,
    /// The target the turn was routed to.
    routed: &'a Target,
    /// Routed refusals since the routed model last answered.
    streak: u32,
}

/// Whether the turn's next request goes to the fallback: it has moved there
/// and not gone back.
pub(super) fn on_fallback(t: &Turn<'_>) -> bool {
    t.fallback
        .as_ref()
        .is_some_and(|f| !std::ptr::eq(t.target, f.routed))
}

impl TurnRunner {
    /// Whether the loop's refused answer goes to its model's fallback: the
    /// switch is on, the turn is on its routed model (a refusal on the
    /// fallback ends the turn), the provider's own fallback did not ride the
    /// request, and the catalog names one. Then the turn moves to it (its own
    /// reservation, its own price), the refused answer's text leaves the
    /// reply, and a `provider.fallback` row says so.
    pub(super) fn fall_back<'a>(
        &self,
        t: &mut Turn<'a>,
        spec: &mut RequestSpec,
        slot: &'a OnceLock<Target>,
        (resp, refused): (&ModelResponse, &Node),
        said_before: (usize, usize),
    ) -> bool {
        // The request just answered, if it was a retry, ends the retry.
        let was_retry = std::mem::take(&mut t.again.pending);
        if resp.stop_reason.as_deref() != Some("refusal") {
            if was_retry {
                spec.fallback = None;
            }
            return false;
        }
        if !self.cfg.model.retries.refusal {
            return false;
        }
        let category = resp
            .stop_details
            .as_ref()
            .and_then(|d| d["category"].as_str());
        let hard = category.is_some_and(|c| {
            let hard = &self.cfg.model.retries.hard_decline_categories;
            hard.iter().any(|h| h == c)
        });
        let refused_at = (refused.loop_index, refused.id.as_str());
        let fell = (!hard && !was_retry && self.retry_same(t, spec, category, refused_at))
            || self.switch_model((t, spec, slot), category, Some(refused_at), hard);
        if fell {
            t.output.truncate(said_before.0);
            t.said.truncate(said_before.1);
        }
        fell
    }

    /// Whether the refused request is made once more on the routed model:
    /// the switch is on, the turn is on its routed model, the turn has a
    /// retry left, and the refusal would otherwise go to a fallback the
    /// catalog names (one the provider's own fallback covers is not ours to
    /// precede). The request is the fallback's shape on the routed model: it
    /// names the model and leaves the refused answer out. Counts no refusal
    /// toward `STAYS_AT` and makes no `FellBack`.
    fn retry_same(
        &self,
        t: &mut Turn<'_>,
        spec: &mut RequestSpec,
        category: Option<&str>,
        (loop_index, refused): (Option<u32>, &str),
    ) -> bool {
        let routed = t.target;
        let entry = self.catalog.get(&routed.model);
        let has_fallback = entry
            .and_then(|e| e.refusal_fallback_model.as_deref())
            .is_some_and(|to| self.catalog.get(to).is_some());
        if !self.cfg.model.retries.refusal_retry
            || on_fallback(t)
            || t.again.used >= RETRIES_PER_TURN
            || !has_fallback
            || crate::compiler::server_fallbacks(spec, entry)
        {
            return false;
        }
        t.again.used += 1;
        t.again.pending = true;
        let n = t.again.used;
        t.record(&fact::turn::RefusalRetried {
            model: &routed.model,
            category,
            loop_index,
            refused,
            n,
        });
        spec.fallback = Some((routed.model.clone(), Some(refused.to_string())));
        true
    }

    /// Whether a call cut at its whole-call bound goes to the model's
    /// fallback, as a refusal does (the owner, 2026-10-25): the same request
    /// would most likely run as long again on the same model. There is no
    /// answer to leave out, so the requests name no refused id. The records
    /// say `timeout` where a refusal's say its category.
    pub(super) fn fall_back_on_timeout<'a>(
        &self,
        t: &mut Turn<'a>,
        spec: &mut RequestSpec,
        slot: &'a OnceLock<Target>,
    ) -> bool {
        if !self.cfg.model.retries.refusal || self.stopping() {
            return false;
        }
        self.switch_model((t, spec, slot), Some("timeout"), None, true)
    }

    /// The switch both triggers share: the catalog's fallback for the turn's
    /// model, its own target and clamp, the record and the row. `refused` is
    /// the refused answer's loop and id, none for a timeout. `stays`: the turn
    /// stays on the fallback however few refusals it has had; any other stays
    /// at the third in a row. A turn on its fallback does not switch again.
    fn switch_model<'a>(
        &self,
        (t, spec, slot): (&mut Turn<'a>, &mut RequestSpec, &'a OnceLock<Target>),
        category: Option<&str>,
        refused: Option<(Option<u32>, &str)>,
        stays: bool,
    ) -> bool {
        let entry = self.catalog.get(&t.target.model);
        if on_fallback(t) || crate::compiler::server_fallbacks(spec, entry) {
            return false;
        }
        let Some((to, cap)) = entry
            .and_then(|e| e.refusal_fallback_model.as_deref())
            .and_then(|to| Some((to, self.catalog.get(to)?.max_output_tokens)))
        else {
            return false;
        };
        // Set once; a later switch finds the same target (the routed target's
        // clamp is the same each time).
        let target = slot.get_or_init(|| Target {
            model: to.to_string(),
            max_tokens: t.target.max_tokens.min(cap),
            ..t.target.clone()
        });
        let routed = t.target;
        let streak = t.fallback.as_ref().map_or(0, |f| f.streak) + 1;
        let latched = stays || streak >= STAYS_AT;
        let fell = t.fallback.take();
        let n = fell.as_ref().map_or(0, |f| f.fallback.refusals) + 1;
        let fallback = TurnFallback {
            from: routed.model.clone(),
            to: to.to_string(),
            category: category.map(str::to_string),
            refusals: n,
            latched,
            ..fell
                .as_ref()
                .map_or_else(TurnFallback::default, |f| f.fallback.clone())
        };
        t.record(&fact::turn::FellBack {
            from: &fallback.from,
            to,
            category,
            loop_index: refused.map_or(Some(t.loops.saturating_sub(1)), |r| r.0),
            refused: refused.map(|r| r.1),
            n,
            latched,
        });
        let ran_on = fell.map_or_else(|| Self::ran_on(t), |f| f.ran_on);
        t.target = target;
        spec.fallback = Some((to.to_string(), refused.map(|r| r.1.to_string())));
        spec.max_tokens = target.max_tokens;
        t.fallback = Some(FellBack {
            fallback,
            ran_on,
            routed,
            streak,
        });
        true
    }

    /// A response is in: the fallback answered when it is the fallback's and
    /// not a refusal; the routed model answered when it is the routed model's
    /// and not a refusal, which ends the run of routed refusals.
    pub(super) fn fallback_answered(t: &mut Turn<'_>, resp: &ModelResponse) {
        let on = on_fallback(t);
        let refusal = resp.stop_reason.as_deref() == Some("refusal");
        if let Some(f) = t.fallback.as_mut().filter(|_| !refusal) {
            if on {
                f.fallback.answers += 1;
                f.fallback.answered = true;
            } else {
                f.streak = 0;
            }
        }
    }

    /// Whether `resp` is a fallback answer that sends the turn back to the
    /// routed model: it is the fallback's and not a refusal, and the turn did
    /// not stay (`latched`).
    pub(super) fn returns_to_routed(t: &Turn<'_>, resp: &ModelResponse) -> bool {
        on_fallback(t)
            && resp.stop_reason.as_deref() != Some("refusal")
            && t.fallback.as_ref().is_some_and(|f| !f.fallback.latched)
    }

    /// The fallback's answer sends the turn back to the routed model
    /// (`returns_to_routed`). Then the next request names the routed model and its
    /// own output cap, and a `provider.routed_back` row says so. The refused
    /// answer stays out by the render's own rule, a later answer of the turn
    /// having replaced it.
    pub(super) fn go_back(
        t: &mut Turn<'_>,
        spec: &mut RequestSpec,
        (resp, answered): (&ModelResponse, &Node),
    ) -> bool {
        if !Self::returns_to_routed(t, resp) {
            return false;
        }
        let Some(f) = t.fallback.as_ref() else {
            return false;
        };
        let (routed, after) = (f.routed, t.target.model.clone());
        t.record(&fact::turn::RoutedBack {
            to: &routed.model,
            after: &after,
            loop_index: answered.loop_index,
        });
        t.target = routed;
        spec.fallback = None;
        spec.max_tokens = routed.max_tokens;
        true
    }
}
