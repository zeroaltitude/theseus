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
//!   `[routing] high_effort_confidence` (0.70); at or below, the turn runs at
//!   `high` and the row says `held_high`, the answered level still recorded.
//!   The gate applies first and the bounds clamp after; where both lower an
//!   answer, the reason is the gate's.
//! - **Only a model that takes effort** (its catalog row's `effort`): for any
//!   other the answer is recorded and the request carries none, as before.
//! - **Sticky on a model without per-message effort** ([`sticky`], theseus-rwpbg.4):
//!   there a change of the top-level effort restarts the messages' cache, so on a
//!   long conversation the request keeps the previous request's level unless
//!   Jev's differs from it by two steps or more. The decision keeps both: `ran`
//!   is the level Jev's answer came to, `sent` the one the request carries.
//! - **The confidence rules are route.v2's**: below the question's `confirm`
//!   bar (its band, the pack's own thresholds), on `unclear`, for a verdict
//!   in shadow or a pinned turn, and for a late verdict (one asked for the
//!   session's last message, which carries its mode, never its effort), the
//!   profile's own effort stands.

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
    /// Under the question's confirm bar.
    Unsure,
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
    /// Jev's level (`ran`) differs from the previous request's by one step on
    /// a long conversation, and the model has no per-message effort: the
    /// request keeps the previous level (`sent`), so its cache is read.
    Sticky,
}

impl EffortReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EffortReason::Applied => "applied",
            EffortReason::Clamped => "clamped",
            EffortReason::HeldHigh => "held_high",
            EffortReason::Unsure => "unsure",
            EffortReason::Unclear => "unclear",
            EffortReason::Fixed => "fixed",
            EffortReason::NoEffort => "no_effort",
            EffortReason::Carried => "carried",
            EffortReason::Recorded => "recorded",
            EffortReason::Sticky => "sticky",
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
                | EffortReason::Clamped
                | EffortReason::HeldHigh
                | EffortReason::Sticky
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

/// The effort rule. `acts` is whether the verdict acts on this turn: live,
/// unpinned, and asked for this turn's own message (`carried` when it is a
/// late verdict of the last one). `high_confidence` is `[routing]
/// high_effort_confidence`: `xhigh` and `max` apply only above it.
pub fn decide(
    bounds: [Effort; 2],
    high_confidence: f64,
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
    if !answer.sure {
        return stand(EffortReason::Unsure);
    }
    if !runs.takes {
        return stand(EffortReason::NoEffort);
    }
    if runs.fixed {
        return stand(EffortReason::Fixed);
    }
    let [lo, hi] = bounds;
    // The gate first: an unsure xhigh or max is a high.
    let held = level >= Effort::Xhigh && answer.confidence <= high_confidence;
    let ran = match held {
        true => Effort::High,
        false => level,
    }
    .clamp(lo, hi);
    EffortDecision {
        reason: match (ran == level, held && ran < level) {
            (true, _) => EffortReason::Applied,
            (false, true) => EffortReason::HeldHigh,
            (false, false) => EffortReason::Clamped,
        },
        ran: Some(ran),
        sent: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: [Effort; 2] = [Effort::Low, Effort::Max];
    const BAR: f64 = 0.70;

    fn a(level: &str, sure: bool) -> EffortAnswer {
        at(level, if sure { 0.9 } else { 0.5 }, sure)
    }

    fn at(level: &str, confidence: f64, sure: bool) -> EffortAnswer {
        EffortAnswer {
            level: level.into(),
            confidence,
            sure,
        }
    }

    fn runs(own: Option<Effort>, fixed: bool, takes: bool) -> Runs {
        Runs { own, fixed, takes }
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
        let d = decide(FULL, 0.9, &at("max", 0.85, true), r, true, false);
        assert_eq!(d.reason, EffortReason::HeldHigh);
        // Not sure at all (under the question's confirm bar): the gate is
        // not reached, the profile's own stands.
        let d = decide(FULL, BAR, &at("max", 0.5, false), r, true, false);
        assert_eq!(d.reason, EffortReason::Unsure);
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
}
