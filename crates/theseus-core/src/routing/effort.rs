//! The effort a turn runs at (route.v3, theseus-qe3v; the owner's decision of
//! 2026-10-07: "jev should also set effort level, not just model"). route.v3
//! asks, in the same request as the mode, how much effort the reply needs
//! (`reply_effort`: low, medium, high, xhigh, max, or `unclear`), and this
//! module decides, purely, what the turn's request carries:
//!
//! - **Jev's answer wins** over the profile's own `effort`, unless the profile
//!   sets `effort_fixed = true`: a profile that names an effort names the one
//!   it runs at without Jev, and a fixed one is the operator's word that it
//!   always runs there (a detour profile kept at `low` for its speed).
//! - **Within `[routing] effort_bounds`** (the full range by default): an
//!   answer outside them is clamped to the nearer bound, and the row says
//!   `clamped`.
//! - **`xhigh` and `max` need confidence** (theseus-p8dy, the owner's decision
//!   of 2026-10-11: Jev may go all the way to max, but only on high evidence):
//!   an answer of either applies only when its confidence is strictly above
//!   `[routing] high_effort_confidence` (0.40, the owner's live bar of
//!   2026-10-22); at or below, the turn runs at `high` and the row says
//!   `held_high`, the answered level still recorded. `xhigh` needs more
//!   (theseus-goq94, the owner's call of 2026-10-23: it doubles the cost for
//!   one solve in thirty): it applies only on a sure answer, so an unsure
//!   `xhigh` runs `high` (`held_high_unsure`) whatever its confidence, and
//!   never says `applied_unsure`. `max` keeps the bar alone, sure or not. The
//!   gate applies first and the bounds clamp after; where both lower an
//!   answer, the reason is the gate's.
//! - **A routed profile's own effort is its floor** (theseus-53ujg.3): on a
//!   profile a routing class chose (not the session's base), the turn runs at
//!   no less than the profile's own `effort`, the level the profile is
//!   defined by. It applies after the gate and before the bounds (the bounds
//!   win), and is not gated: an `xhigh` floor is the profile's setting, not
//!   Jev's guess. The row says `floored` only when it raised the level. On
//!   the base profile Jev may still go below its own.
//! - **Only a model that takes effort** (its catalog row's `effort`): for any
//!   other the answer is recorded and the request carries none, as before.
//! - **Sticky on a model without per-message effort** ([`sticky`], theseus-rwpbg.4):
//!   there a change of the top-level effort restarts the messages' cache, so on a
//!   long conversation the request keeps the previous request's level unless
//!   Jev's differs from it by two steps or more. The decision keeps both: `ran`
//!   is the level Jev's answer came to, `sent` the one the request carries.
//!   `sticky_unsure` stands for the same hold of an `applied_unsure` or a
//!   `held_high_unsure` answer.
//! - **The effort has its own bar, not the model switch's** (theseus-7pm8t):
//!   an answer below the question's `confirm` bar (its band, the pack's own
//!   thresholds, which a model switch needs) still applies when its
//!   confidence is at or above `[routing] effort_confidence` (0.35): an
//!   effort level costs far less than a cold cache. Under it the profile's
//!   own effort stands (`unsure`). Such an answer goes through the gate, the
//!   floor and the bounds as a sure one does; the row says `applied_unsure`
//!   only where the answer's own level ran, and the gate's, the floor's and
//!   the bounds' reasons win where one of them moved it.
//! - **Under `[routing] effort_floor_confidence` an answer applies nothing**
//!   (theseus-l7i9k, regress R4; unset is off, and it ships off). The turn
//!   runs at the profile's own effort and the row says `gated`, the answered
//!   level still recorded. It sits after `fixed` and `no_effort` (which say
//!   more) and before the gate, the floor and the bounds, so it stops all
//!   three of `floored`, `applied_unsure` and `held_high_unsure` at once. On a
//!   routed profile the profile's own effort IS the floor, so a `floored`
//!   answer that is gated runs at the same level it would have: only its
//!   reason changes.
//! - **The rest are route.v2's**: on `unclear`, for a verdict in shadow or a
//!   pinned turn, and for a late verdict (one asked for the session's last
//!   message, which carries its mode, never its effort), the profile's own
//!   effort stands.

use crate::config::Effort;

/// route.v3's effort question, by its id in the pack.
pub const QUESTION: &str = "reply_effort";

/// route.v3's answer about the reply's effort, as the turn reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct EffortAnswer {
    /// The option Jev chose: a level, or `unclear`.
    pub level: String,
    pub confidence: f64,
    /// Its band reached the question's `confirm` bar (act or confirm).
    pub sure: bool,
}

/// Why a turn runs at the effort it runs at (the `route.decided` row's
/// `effort_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortReason {
    /// Jev's level, within the bounds.
    Applied,
    /// Jev's level, clamped to the nearer bound.
    Clamped,
    /// Jev answered `xhigh` or `max` at or under `high_effort_confidence`:
    /// the turn runs at `high` (theseus-p8dy).
    HeldHigh,
    /// [`HeldHigh`](EffortReason::HeldHigh) for an answer under the question's
    /// confirm bar: `held_high` keeps meaning the sure one (theseus-t7b96).
    /// An unsure `xhigh` is always held, above the bar too (theseus-goq94).
    HeldHighUnsure,
    /// Under the question's confirm bar and under `effort_confidence`.
    Unsure,
    /// Jev's level, within the bounds, from an answer under the question's
    /// confirm bar but at or above `effort_confidence` (theseus-7pm8t).
    AppliedUnsure,
    /// Jev answered `unclear`.
    Unclear,
    /// The profile fixes its effort (`effort_fixed`).
    Fixed,
    /// The turn's model takes no effort.
    NoEffort,
    /// A late verdict of the session's last message: its mode carries, its
    /// effort does not.
    Carried,
    /// Recorded only: routing in shadow, or a turn the owner pinned.
    Recorded,
    /// Jev's level was under the routed profile's own effort, which the turn
    /// runs at instead (theseus-53ujg.3).
    Floored,
    /// The answer's confidence was under `effort_floor_confidence`: it applies
    /// nothing, and the profile's own effort stands (theseus-l7i9k).
    Gated,
    /// Jev's level (`ran`) differs from the previous request's by one step on
    /// a long conversation, and the model has no per-message effort: the
    /// request keeps the previous level (`sent`), so its cache is read.
    Sticky,
    /// [`Sticky`](EffortReason::Sticky) holding an `applied_unsure` answer:
    /// the hold keeps the bar that admitted it visible (theseus-8zgji).
    StickyUnsure,
    /// A later answer that disagrees with a sticky hold ended it early: the
    /// request carries Jev's level (theseus-u5a37, [`reverts`]).
    StickyReverted,
    /// route.v10: the model's own default effort ran, because Jev answered
    /// `default` or its answer was under the question's confirm bar.
    ModelDefault,
}

impl EffortReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EffortReason::Applied => "applied",
            EffortReason::Clamped => "clamped",
            EffortReason::HeldHigh => "held_high",
            EffortReason::HeldHighUnsure => "held_high_unsure",
            EffortReason::Unsure => "unsure",
            EffortReason::AppliedUnsure => "applied_unsure",
            EffortReason::Unclear => "unclear",
            EffortReason::Fixed => "fixed",
            EffortReason::NoEffort => "no_effort",
            EffortReason::Carried => "carried",
            EffortReason::Recorded => "recorded",
            EffortReason::Sticky => "sticky",
            EffortReason::StickyUnsure => "sticky_unsure",
            EffortReason::StickyReverted => "sticky_reverted",
            EffortReason::Floored => "floored",
            EffortReason::Gated => "gated",
            EffortReason::ModelDefault => "model_default",
        }
    }
}

/// What the turn's profile and model say about effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Runs {
    /// The profile's own `effort` (none: the model's default).
    pub own: Option<Effort>,
    /// The profile's `effort_fixed`.
    pub fixed: bool,
    /// The model's catalog row takes effort.
    pub takes: bool,
    /// The least the turn runs at: a routed profile's own effort, none on the
    /// session's base profile.
    pub floor: Option<Effort>,
}

/// The rule's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffortDecision {
    pub reason: EffortReason,
    /// The effort the request carries: Jev's when applied, else the
    /// profile's own; none for a model that takes no effort.
    pub ran: Option<Effort>,
    /// The level the request carries when [`sticky`] held it at the previous
    /// request's, else none (it carries `ran`).
    pub sent: Option<Effort>,
}

impl EffortDecision {
    /// Jev's answer set the turn's effort.
    pub fn applied(&self) -> bool {
        matches!(
            self.reason,
            EffortReason::Applied
                | EffortReason::AppliedUnsure
                | EffortReason::Clamped
                | EffortReason::HeldHigh
                | EffortReason::HeldHighUnsure
                | EffortReason::Sticky
                | EffortReason::StickyUnsure
                | EffortReason::StickyReverted
                | EffortReason::Floored
                | EffortReason::ModelDefault
        )
    }
}

/// A level's name, as the pack's option and the config write it.
pub fn name(e: Effort) -> &'static str {
    match e {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max => "max",
    }
}

/// A level from its name; none for `unclear` or any other option.
pub fn parse(level: &str) -> Option<Effort> {
    [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ]
    .into_iter()
    .find(|e| name(*e) == level)
}

/// The effort bars of `[routing]`: `effort_confidence`, the least an answer
/// whose band is unsure applies at, `high_effort_confidence`, which `xhigh`
/// and `max` must pass, and `effort_floor_confidence`, under which no answer
/// applies when it is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bars {
    pub effort: f64,
    pub high: f64,
    /// `[routing] effort_floor_confidence`: an answer under it applies
    /// nothing (theseus-l7i9k). None is off.
    pub floor: Option<f64>,
}

/// The effort rule. `acts` is whether the verdict acts on this turn: live,
/// unpinned, and asked for this turn's own message (`carried` when it is a
/// late verdict of the last one).
pub fn decide(
    bounds: [Effort; 2],
    bars: Bars,
    answer: &EffortAnswer,
    runs: Runs,
    acts: bool,
    carried: bool,
) -> EffortDecision {
    let own = runs.takes.then_some(runs.own).flatten();
    let stand = |reason| EffortDecision {
        reason,
        ran: own,
        sent: None,
    };
    if !acts {
        return stand(EffortReason::Recorded);
    }
    if carried {
        return stand(EffortReason::Carried);
    }
    let Some(level) = parse(&answer.level) else {
        return stand(EffortReason::Unclear);
    };
    if !answer.sure && answer.confidence < bars.effort {
        return stand(EffortReason::Unsure);
    }
    if !runs.takes {
        return stand(EffortReason::NoEffort);
    }
    if runs.fixed {
        return stand(EffortReason::Fixed);
    }
    // The floor bar (off by default): an answer under it moves nothing, not
    // up by `applied_unsure`, `held_high_unsure` or the routed profile's
    // floor, and not down.
    if bars.floor.is_some_and(|f| answer.confidence < f) {
        return stand(EffortReason::Gated);
    }
    let [lo, hi] = bounds;
    // The gate first: an xhigh on an unsure answer, or any xhigh or max at or
    // under the bar, is a high.
    // `xhigh` also needs a sure answer (theseus-goq94); `max` keeps the bar alone.
    let held = match level {
        Effort::Xhigh => !answer.sure || answer.confidence <= bars.high,
        Effort::Max => answer.confidence <= bars.high,
        _ => false,
    };
    let gated = match held {
        true => Effort::High,
        false => level,
    };
    // Then the floor, ungated; the bounds last.
    let ran = runs.floor.map_or(gated, |f| gated.max(f)).clamp(lo, hi);
    EffortDecision {
        reason: match (ran == level, held && ran < level) {
            _ if ran > gated.clamp(lo, hi) => EffortReason::Floored,
            (true, _) if answer.sure => EffortReason::Applied,
            (true, _) => EffortReason::AppliedUnsure,
            (false, true) if answer.sure => EffortReason::HeldHigh,
            (false, true) => EffortReason::HeldHighUnsure,
            (false, false) => EffortReason::Clamped,
        },
        ran: Some(ran),
        sent: None,
    }
}

/// route.v10's effort question, by its id in the pack: route.v9's
/// `reply_effort` with `leave` renamed `default`.
pub const OPEN_QUESTION: &str = "effort";

/// What the turn's profile and model say about effort, for route.v10, where
/// Jev's answer is the effort: no bounds, no high-effort gate, no floor, no
/// fixed profile (a pinned turn is not asked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenRuns {
    /// The profile's own `effort`, which a turn Jev did not decide runs at.
    pub own: Option<Effort>,
    /// The model's own default effort (`CatalogEntry.default_effort`).
    pub default: Option<Effort>,
    /// The model's catalog row takes effort.
    pub takes: bool,
}

/// route.v10's effort rule. Jev's level, when the answer reached the bar, is
/// what the request carries, whatever the level and wherever the model sits;
/// `default`, or an answer under the bar, is the model's own default effort.
pub fn decide_open(
    runs: OpenRuns,
    answer: &EffortAnswer,
    acts: bool,
    carried: bool,
) -> EffortDecision {
    let own = runs.takes.then_some(runs.own).flatten();
    let stand = |reason| EffortDecision {
        reason,
        ran: own,
        sent: None,
    };
    if !acts {
        return stand(EffortReason::Recorded);
    }
    if carried {
        return stand(EffortReason::Carried);
    }
    if !runs.takes {
        return EffortDecision {
            reason: EffortReason::NoEffort,
            ran: None,
            sent: None,
        };
    }
    match parse(&answer.level).filter(|_| answer.sure) {
        Some(level) => EffortDecision {
            reason: EffortReason::Applied,
            ran: Some(level),
            sent: None,
        },
        // `default`, or an answer under its bar: the model's own.
        None => EffortDecision {
            reason: EffortReason::ModelDefault,
            ran: runs.default,
            sent: None,
        },
    }
}

/// The sticky rule. A model without per-message effort pays a rewrite of its
/// messages' cache for every change of the top-level effort, so `asked` (Jev's
/// level after the bounds) is held at `previous`, the level the session's last
/// request carried, when it differs by one step. It is not held when there is
/// no previous level (a first request, or a model switch, which rewrites
/// anyway), when the request is under `floor` tokens (little cache to lose), or
/// when `asked` is two steps or more away. Returns the level held at.
pub fn sticky(asked: Effort, previous: Option<Effort>, tokens: u64, floor: u64) -> Option<Effort> {
    previous.filter(|p| *p != asked && tokens >= floor && (*p as i8).abs_diff(asked as i8) < 2)
}

/// The kind of sticky hold the session's last request carried: the window a
/// later answer may end early (theseus-u5a37).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// A held `applied` or `held_high` answer.
    Sticky,
    /// A held `applied_unsure` or `held_high_unsure` answer.
    Unsure,
}

/// Whether an answer that disagrees with the held level ends the hold early.
/// Inside a `sticky` window the answer must be `sure` (its band reached the
/// question's own `confirm` bar); inside a `sticky_unsure` window the lower bar
/// is enough, `effort_confidence` (the owner, 2026-10-22: "a lower bar to
/// revert it for sticky_unsure"). No window, no revert.
pub fn reverts(window: Option<Hold>, answer: &EffortAnswer, effort_bar: f64) -> bool {
    match window {
        None => false,
        Some(Hold::Sticky) => answer.sure,
        Some(Hold::Unsure) => answer.sure || answer.confidence >= effort_bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: [Effort; 2] = [Effort::Low, Effort::Max];
    const BAR: Bars = Bars {
        effort: 0.35,
        high: 0.70,
        floor: None,
    };

    /// A sure answer at 0.9, an unsure one under `BAR.effort`.
    fn a(level: &str, sure: bool) -> EffortAnswer {
        at(level, if sure { 0.9 } else { 0.3 }, sure)
    }

    fn at(level: &str, confidence: f64, sure: bool) -> EffortAnswer {
        EffortAnswer {
            level: level.into(),
            confidence,
            sure,
        }
    }

    fn runs(own: Option<Effort>, fixed: bool, takes: bool) -> Runs {
        Runs {
            own,
            fixed,
            takes,
            floor: None,
        }
    }

    /// Jev's sure answer sets the effort over the profile's own, for a model
    /// that takes effort; every level is a pack option.
    #[test]
    fn a_sure_answer_sets_the_effort_over_the_profiles_own() {
        for e in [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
        ] {
            assert_eq!(parse(name(e)), Some(e));
            let d = decide(
                FULL,
                BAR,
                &a(name(e), true),
                runs(Some(Effort::Low), false, true),
                true,
                false,
            );
            assert_eq!((d.reason, d.ran), (EffortReason::Applied, Some(e)), "{e:?}");
            assert!(d.applied());
        }
        let d = decide(
            FULL,
            BAR,
            &a("high", true),
            runs(None, false, true),
            true,
            false,
        );
        assert_eq!(d.ran, Some(Effort::High), "over the model's default");
        assert_eq!(parse("unclear"), None);
    }

    /// The bounds clamp to the nearer one, and say so.
    #[test]
    fn the_bounds_clamp_an_answer_outside_them() {
        let b = [Effort::Medium, Effort::High];
        let r = runs(None, false, true);
        for (level, ran, reason) in [
            ("low", Effort::Medium, EffortReason::Clamped),
            ("medium", Effort::Medium, EffortReason::Applied),
            ("high", Effort::High, EffortReason::Applied),
            ("xhigh", Effort::High, EffortReason::Clamped),
            ("max", Effort::High, EffortReason::Clamped),
        ] {
            let d = decide(b, BAR, &a(level, true), r, true, false);
            assert_eq!((d.reason, d.ran), (reason, Some(ran)), "{level}");
            assert!(d.applied());
        }
    }

    /// Below the bar, `unclear`, a fixed profile, a model without effort, a
    /// carried late verdict, and shadow: the profile's own stands (none for
    /// a model that takes no effort).
    #[test]
    fn otherwise_the_profiles_own_effort_stands() {
        let low = Some(Effort::Low);
        let cases = [
            (
                a("high", false),
                runs(low, false, true),
                true,
                false,
                EffortReason::Unsure,
                low,
            ),
            (
                a("unclear", true),
                runs(low, false, true),
                true,
                false,
                EffortReason::Unclear,
                low,
            ),
            (
                a("max", true),
                runs(low, true, true),
                true,
                false,
                EffortReason::Fixed,
                low,
            ),
            (
                a("max", true),
                runs(low, false, false),
                true,
                false,
                EffortReason::NoEffort,
                None,
            ),
            (
                a("max", true),
                runs(low, false, true),
                true,
                true,
                EffortReason::Carried,
                low,
            ),
            (
                a("max", true),
                runs(None, false, true),
                false,
                false,
                EffortReason::Recorded,
                None,
            ),
        ];
        for (answer, r, acts, carried, reason, ran) in cases {
            let d = decide(FULL, BAR, &answer, r, acts, carried);
            assert_eq!((d.reason, d.ran), (reason, ran), "{answer:?} {r:?}");
            assert!(!d.applied());
        }
    }

    /// `xhigh` and `max` need confidence strictly above the bar: at it they
    /// run `high`, and the row says `held_high` with the answer kept; under
    /// `max` nothing is gated (theseus-p8dy).
    #[test]
    fn xhigh_and_max_apply_only_above_the_confidence_bar() {
        let r = runs(None, false, true);
        for (level, c, ran, reason) in [
            ("xhigh", 0.70, Effort::High, EffortReason::HeldHigh),
            ("xhigh", 0.71, Effort::Xhigh, EffortReason::Applied),
            ("max", 0.70, Effort::High, EffortReason::HeldHigh),
            ("max", 0.65, Effort::High, EffortReason::HeldHigh),
            ("max", 0.71, Effort::Max, EffortReason::Applied),
            ("high", 0.61, Effort::High, EffortReason::Applied),
            ("medium", 0.61, Effort::Medium, EffortReason::Applied),
            ("low", 0.61, Effort::Low, EffortReason::Applied),
        ] {
            let d = decide(FULL, BAR, &at(level, c, true), r, true, false);
            assert_eq!((d.reason, d.ran), (reason, Some(ran)), "{level} at {c}");
            assert!(d.applied(), "{level} at {c}");
        }
        assert_eq!(EffortReason::HeldHigh.as_str(), "held_high");
        // A raised bar moves the line; a bar of 1.0 holds every max.
        let raised = Bars { high: 0.9, ..BAR };
        let d = decide(FULL, raised, &at("max", 0.85, true), r, true, false);
        assert_eq!(d.reason, EffortReason::HeldHigh);
        // Under the question's confirm bar and under `effort_confidence`:
        // the gate is not reached, the profile's own stands.
        let d = decide(FULL, BAR, &at("max", 0.3, false), r, true, false);
        assert_eq!(d.reason, EffortReason::Unsure);
    }

    /// The wire's `effort_reason` doc names every word `as_str` gives, in its
    /// order (theseus-t7b96).
    #[test]
    fn the_wires_doc_lists_every_effort_reason_word() {
        let doc = include_str!("../../../theseus-protocol/src/route.rs");
        let doc = &doc[doc.find("Why the turn ran at its effort").unwrap()..];
        let doc = &doc[..doc.find("pub effort_reason").unwrap()];
        let mut at = 0;
        for r in [
            EffortReason::Applied,
            EffortReason::Clamped,
            EffortReason::HeldHigh,
            EffortReason::HeldHighUnsure,
            EffortReason::Unsure,
            EffortReason::AppliedUnsure,
            EffortReason::Unclear,
            EffortReason::Fixed,
            EffortReason::NoEffort,
            EffortReason::Carried,
            EffortReason::Recorded,
            EffortReason::Sticky,
            EffortReason::StickyUnsure,
            EffortReason::StickyReverted,
            EffortReason::Floored,
            EffortReason::Gated,
        ] {
            let word = format!("`{}`", r.as_str());
            let i = doc[at..].find(&word);
            assert!(
                i.is_some(),
                "{word} is missing from the doc, or out of order"
            );
            at += i.unwrap();
        }
    }

    /// A held answer says whether it was sure: `held_high` for one that
    /// reached the question's confirm bar, `held_high_unsure` for one that
    /// only cleared `effort_confidence` (theseus-t7b96).
    #[test]
    fn an_unsure_max_held_high_says_so() {
        let r = runs(None, false, true);
        for (level, c, sure, reason) in [
            ("max", 0.5, false, EffortReason::HeldHighUnsure),
            ("xhigh", 0.4, false, EffortReason::HeldHighUnsure),
            ("max", 0.65, true, EffortReason::HeldHigh),
        ] {
            let d = decide(FULL, BAR, &at(level, c, sure), r, true, false);
            assert_eq!(
                (d.reason, d.ran),
                (reason, Some(Effort::High)),
                "{level} at {c}"
            );
            assert!(d.applied());
        }
        assert_eq!(EffortReason::HeldHighUnsure.as_str(), "held_high_unsure");
    }

    /// The gate applies first and the bounds clamp after: where both lower an
    /// answer the reason is the gate's, where only the bounds do it is
    /// `clamped`, and a bound above `high` raises a held answer back.
    #[test]
    fn the_gate_runs_before_the_bounds_and_names_the_reason_when_both_lower() {
        let r = runs(None, false, true);
        let b = [Effort::Low, Effort::Medium];
        let d = decide(b, BAR, &at("max", 0.65, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::HeldHigh, Some(Effort::Medium))
        );
        let d = decide(b, BAR, &at("max", 0.9, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Clamped, Some(Effort::Medium))
        );
        let d = decide(b, BAR, &at("high", 0.9, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Clamped, Some(Effort::Medium))
        );
        // A floor at xhigh lifts the held `high` back to the answered level.
        let up = [Effort::Xhigh, Effort::Max];
        let d = decide(up, BAR, &at("xhigh", 0.65, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Applied, Some(Effort::Xhigh))
        );
        // The gate lowered a max, and the floor lifted it only to xhigh.
        let d = decide(up, BAR, &at("max", 0.65, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::HeldHigh, Some(Effort::Xhigh))
        );
    }
    /// A routed profile's own effort is the least it runs at: a sure answer
    /// under it runs at it (`floored`), one over it applies, the floor is not
    /// gated, and the bounds win; with no floor (the base profile) Jev may go
    /// below the profile's own (theseus-53ujg.3).
    #[test]
    fn a_routed_profiles_own_effort_is_its_floor() {
        let floored = |own| Runs {
            floor: Some(own),
            ..runs(Some(own), false, true)
        };
        let high = floored(Effort::High);
        for (bounds, level, c, r, ran, reason) in [
            (
                FULL,
                "medium",
                0.9,
                high,
                Effort::High,
                EffortReason::Floored,
            ),
            (
                FULL,
                "xhigh",
                0.9,
                high,
                Effort::Xhigh,
                EffortReason::Applied,
            ),
            (FULL, "high", 0.9, high, Effort::High, EffortReason::Applied),
            // The gate held a max at high; an xhigh floor is no guess.
            (
                FULL,
                "max",
                0.6,
                floored(Effort::Xhigh),
                Effort::Xhigh,
                EffortReason::Floored,
            ),
            // The bounds win, and the level the bounds leave is Jev's.
            (
                [Effort::Low, Effort::Medium],
                "medium",
                0.9,
                high,
                Effort::Medium,
                EffortReason::Applied,
            ),
            (
                [Effort::Low, Effort::Medium],
                "low",
                0.9,
                high,
                Effort::Medium,
                EffortReason::Floored,
            ),
            // No floor: the base profile goes below its own.
            (
                FULL,
                "low",
                0.9,
                runs(Some(Effort::High), false, true),
                Effort::Low,
                EffortReason::Applied,
            ),
        ] {
            let d = decide(bounds, BAR, &at(level, c, true), r, true, false);
            assert_eq!((d.reason, d.ran), (reason, Some(ran)), "{level} {r:?}");
            assert!(d.applied());
        }
        assert_eq!(EffortReason::Floored.as_str(), "floored");
    }

    /// The owner's live bars (2026-10-22): 0.35 to apply an unsure answer,
    /// 0.40 for `xhigh` and `max`.
    const LIVE: Bars = Bars {
        effort: 0.35,
        high: 0.40,
        floor: None,
    };

    /// The Terminal-Bench case (theseus-7pm8t): Jev asked for `max` at 0.42
    /// with its band under the confirm bar, and the turn ran at the profile's
    /// own. On the effort's own bar it runs at `max`, and the row says so.
    #[test]
    fn a_max_answer_at_0_42_with_an_unsure_band_runs_at_max() {
        let r = runs(Some(Effort::Medium), false, true);
        let d = decide(FULL, LIVE, &at("max", 0.42, false), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::AppliedUnsure, Some(Effort::Max))
        );
        assert!(d.applied());
        assert_eq!(d.reason.as_str(), "applied_unsure");
        // At the bar exactly it applies too; a sure answer is unaffected.
        let d = decide(FULL, LIVE, &at("medium", 0.35, false), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::AppliedUnsure, Some(Effort::Medium))
        );
        let d = decide(FULL, LIVE, &at("max", 0.42, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Applied, Some(Effort::Max))
        );
    }

    /// The owner's call of 2026-10-23 (theseus-goq94): `xhigh` costs about
    /// twice a `high` for one solve in thirty, so it applies only on a sure
    /// answer. An unsure `xhigh` runs `high` even above the bar and never says
    /// `applied_unsure`; a sure one applies, and `max` keeps its bar alone.
    #[test]
    fn an_unsure_xhigh_runs_high_even_above_the_bar() {
        let r = runs(Some(Effort::Medium), false, true);
        let d = decide(FULL, LIVE, &at("xhigh", 0.45, false), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::HeldHighUnsure, Some(Effort::High))
        );
        assert_eq!(d.reason.as_str(), "held_high_unsure");
        let d = decide(FULL, LIVE, &at("xhigh", 0.45, true), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Applied, Some(Effort::Xhigh))
        );
        // Only xhigh: an unsure max above the bar still runs at max.
        let d = decide(FULL, LIVE, &at("max", 0.45, false), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::AppliedUnsure, Some(Effort::Max))
        );
    }

    /// Under `effort_confidence` an unsure answer leaves the profile's own.
    #[test]
    fn an_unsure_answer_under_effort_confidence_keeps_the_profiles_effort() {
        let low = Some(Effort::Low);
        let d = decide(
            FULL,
            LIVE,
            &at("max", 0.30, false),
            runs(low, false, true),
            true,
            false,
        );
        assert_eq!((d.reason, d.ran), (EffortReason::Unsure, low));
        assert!(!d.applied());
    }

    /// The gate holds an unsure answer as it holds a sure one: between the
    /// two bars an `xhigh` runs `high`. The floor and the bounds name their
    /// own reasons over `applied_unsure`.
    #[test]
    fn an_unsure_xhigh_between_the_bars_runs_high() {
        let bars = Bars {
            effort: 0.35,
            high: 0.50,
            floor: None,
        };
        let r = runs(None, false, true);
        let d = decide(FULL, bars, &at("xhigh", 0.42, false), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::HeldHighUnsure, Some(Effort::High))
        );
        let b = [Effort::Low, Effort::Medium];
        let d = decide(b, bars, &at("high", 0.42, false), r, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Clamped, Some(Effort::Medium))
        );
        let floored = Runs {
            floor: Some(Effort::High),
            ..runs(Some(Effort::High), false, true)
        };
        let d = decide(FULL, bars, &at("low", 0.42, false), floored, true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::Floored, Some(Effort::High))
        );
    }

    /// The revert's bars (theseus-u5a37): a `sticky` window needs a sure
    /// answer, a `sticky_unsure` one `effort_confidence`; no window, none.
    #[test]
    fn a_sticky_hold_reverts_on_its_own_bar() {
        for (window, sure, c, reverts_) in [
            (Some(Hold::Sticky), true, 0.60, true),
            (Some(Hold::Sticky), true, 0.95, true),
            (Some(Hold::Sticky), false, 0.59, false),
            (Some(Hold::Sticky), false, 0.35, false),
            (Some(Hold::Unsure), false, 0.30, false),
            (Some(Hold::Unsure), false, 0.34, false),
            (Some(Hold::Unsure), false, 0.35, true),
            (Some(Hold::Unsure), false, 0.59, true),
            (Some(Hold::Unsure), true, 0.60, true),
            (None, true, 0.95, false),
            (None, false, 0.59, false),
        ] {
            let got = reverts(window, &at("medium", c, sure), 0.35);
            assert_eq!(got, reverts_, "{window:?} sure={sure} at {c}");
        }
        assert_eq!(EffortReason::StickyReverted.as_str(), "sticky_reverted");
        let d = EffortDecision {
            reason: EffortReason::StickyReverted,
            ran: Some(Effort::Medium),
            sent: None,
        };
        assert!(d.applied());
    }

    /// `effort_floor_confidence` (theseus-l7i9k): under it an answer applies
    /// nothing and the profile's own effort stands, for each of the three
    /// reasons it exists to stop; at it, nothing changes; unset, off.
    #[test]
    fn an_answer_under_the_floor_bar_applies_nothing() {
        let gated = Bars {
            floor: Some(0.6),
            ..LIVE
        };
        let high = Some(Effort::High);
        // (answer, runs, the reason without the bar)
        let cases = [
            (
                at("max", 0.45, false),
                runs(high, false, true),
                EffortReason::AppliedUnsure,
            ),
            (
                at("xhigh", 0.5, false),
                runs(high, false, true),
                EffortReason::HeldHighUnsure,
            ),
            (
                at("medium", 0.55, false),
                Runs {
                    floor: high,
                    ..runs(high, false, true)
                },
                EffortReason::Floored,
            ),
        ];
        for (answer, r, was) in cases {
            let off = decide(FULL, LIVE, &answer, r, true, false);
            assert_eq!(off.reason, was, "{answer:?}");
            let on = decide(FULL, gated, &answer, r, true, false);
            assert_eq!(
                (on.reason, on.ran, on.sent),
                (EffortReason::Gated, high, None),
                "{answer:?}"
            );
            assert!(!on.applied());
            assert_eq!(EffortReason::Gated.as_str(), "gated");
        }
        // The profile has no effort of its own: none stands.
        let d = decide(
            FULL,
            gated,
            &at("max", 0.45, false),
            runs(None, false, true),
            true,
            false,
        );
        assert_eq!((d.reason, d.ran), (EffortReason::Gated, None));
    }

    /// At the bar, or over it, the answer goes through the rest as it did;
    /// the reasons that say more (`fixed`, `no_effort`, `unsure`, `unclear`)
    /// still come first.
    #[test]
    fn an_answer_at_the_floor_bar_and_the_reasons_before_it_are_unchanged() {
        let gated = Bars {
            floor: Some(0.6),
            ..BAR
        };
        let r = runs(Some(Effort::High), false, true);
        for (conf, sure) in [(0.6, true), (0.9, true)] {
            let a = at("medium", conf, sure);
            assert_eq!(
                decide(FULL, gated, &a, r, true, false),
                decide(FULL, BAR, &a, r, true, false),
                "{conf}"
            );
        }
        let under = at("low", 0.5, false);
        for (r, want) in [
            (runs(Some(Effort::High), true, true), EffortReason::Fixed),
            (
                runs(Some(Effort::High), false, false),
                EffortReason::NoEffort,
            ),
        ] {
            assert_eq!(decide(FULL, gated, &under, r, true, false).reason, want);
        }
        assert_eq!(
            decide(FULL, gated, &at("low", 0.30, false), r, true, false).reason,
            EffortReason::Unsure
        );
        assert_eq!(
            decide(FULL, gated, &at("unclear", 0.2, false), r, true, false).reason,
            EffortReason::Unclear
        );
        assert_eq!(
            decide(FULL, gated, &under, r, false, false).reason,
            EffortReason::Recorded
        );
    }
}
