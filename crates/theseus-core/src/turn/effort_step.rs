//! A model that refuses per-message effort, found out at run time
//! (theseus-posww). `compiler::effort::default_of` names the models (the
//! catalog's `per_message_effort`) that take the effort-only system messages,
//! and a provider can change what it accepts. The 400s of 2026-10-11 and
//! 2026-10-21 (`messages.N.output_config: Extra inputs are not permitted`)
//! were requests sent without the beta (theseus-blzt6), so a refusal is now
//! rare. A 400 that names `output_config` on a request that carries such a
//! message is asked again once, in the same turn, with those messages left
//! out and the turn's level at the top; the model is marked for
//! [`REFUSAL_HOLDS_MS`], so no request within the hour carries one, and a
//! `provider.effort_refused` row says so. After the hour the next request
//! tries the messages again; a restart forgets the mark sooner.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;
use crate::routing::effort::{self, EffortDecision, EffortReason};

/// How long a refusal holds: the provider may change its mind.
const REFUSAL_HOLDS_MS: u64 = 3_600_000;

/// The models this daemon found to refuse per-message effort, each with the
/// kernel clock's time (ms) it was marked.
#[derive(Default)]
pub struct EffortRefusals(Mutex<BTreeMap<String, u64>>);

impl EffortRefusals {
    fn mark(&self, model: &str, now_ms: u64) {
        self.0.lock().unwrap().insert(model.to_string(), now_ms);
    }

    /// The models marked within [`REFUSAL_HOLDS_MS`] of `now_ms`.
    fn all(&self, now_ms: u64) -> Vec<String> {
        let held = self.0.lock().unwrap();
        held.iter()
            .filter(|(_, at)| now_ms < at.saturating_add(REFUSAL_HOLDS_MS))
            .map(|(m, _)| m.clone())
            .collect()
    }
}

impl TurnRunner {
    /// The models a request spec says take no per-message effort.
    pub(super) fn effort_refused(&self) -> Vec<String> {
        self.effort_refusals.all(self.kernel.now_ms())
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
        self.effort_refusals.mark(&model, self.kernel.now_ms());
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

    /// Jev's decided effort made sticky (theseus-rwpbg.4) for a model that
    /// takes no per-message effort: a one-step change from the level the
    /// session's last request carried is held at that level, on a request of
    /// at least `[cache] keep_warm_min_tokens`, since a change of the top-level
    /// level restarts the messages' cache. `tokens` is the compile's own
    /// estimate of the whole request, the measure keep-warm applies to the same
    /// floor, so no token count is added. The decision says `sticky` and keeps
    /// both levels; an `applied_unsure` or `held_high_unsure` answer held
    /// says `sticky_unsure`, so the row still shows which bar admitted it. A
    /// checkpoint's raise is never held: the work's own evidence is what the
    /// rule waits for (theseus-53ujg.3). A hold ends early when a later answer
    /// disagrees and is sure enough for the window it is in (`effort::reverts`,
    /// theseus-u5a37): the request carries Jev's level and the reason is
    /// `sticky_reverted`; a checkpoint never reverts.
    pub(super) fn stick(
        &self,
        t: &Turn<'_>,
        spec: &RequestSpec,
        d: EffortDecision,
        answer: &effort::EffortAnswer,
        (model, tokens): (&str, u64),
        checkpoint: bool,
    ) -> EffortDecision {
        let entry = self.catalog.get(model);
        let held = d
            .applied()
            .then_some(d.ran)
            .flatten()
            .filter(|_| entry.is_some_and(|e| e.caches))
            .filter(|_| !crate::compiler::effort::takes(spec, entry, model))
            .and_then(|asked| {
                let previous = self.keep_warm.last_effort(t.tc.session_id, model);
                effort::sticky(asked, previous, tokens, self.cfg.cache.keep_warm_min_tokens)
                    .filter(|held| !(checkpoint && asked > *held))
            });
        let window = self.keep_warm.last_hold(t.tc.session_id, model);
        match held {
            Some(_) if !checkpoint && effort::reverts(window, answer) => EffortDecision {
                reason: EffortReason::StickyReverted,
                ..d
            },
            Some(sent) => EffortDecision {
                reason: EffortReason::Sticky,
                sent: Some(sent),
                ..d
            },
            None => d,
        }
    }

    /// Records the level a call's request carried: what the next turn's
    /// sticky rule and a keep-warm read of this session replay. A detour's
    /// request is outside the session's compilation and records none.
    pub(super) fn note_sent(&self, t: &Turn<'_>, spec: &RequestSpec, model: &str) {
        if t.route.keeps.is_none() {
            let hold = t
                .route
                .effort
                .filter(|d| d.sent == spec.effort)
                .and_then(|d| match d.reason {
                    EffortReason::Sticky => Some(effort::Hold::Sticky),
                    _ => None,
                });
            self.keep_warm
                .sent(t.tc.session_id, model, spec.effort, hold);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_mark_lapses_after_an_hour() {
        let r = EffortRefusals::default();
        let t = 1_000_000;
        r.mark("claude-sonnet-5-5", t);
        r.mark("claude-fable-5-1", t + 60_000);
        assert_eq!(r.all(t), ["claude-fable-5-1", "claude-sonnet-5-5"]);
        let lapse = t + REFUSAL_HOLDS_MS;
        assert_eq!(
            r.all(lapse - 1),
            ["claude-fable-5-1", "claude-sonnet-5-5"],
            "held to the last ms"
        );
        assert_eq!(r.all(lapse), ["claude-fable-5-1"], "its own mark");
        assert!(r.all(lapse + 60_000).is_empty());
    }
}
