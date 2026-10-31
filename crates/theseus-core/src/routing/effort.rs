//! The effort a turn runs at (route.v3, theseus-qe3v; the owner's decision of
//! 2026-10-07: "jev should also set effort level, not just model"; route.v10,
//! theseus-h66v4: Jev's level is the effort, "operator knows best"). The route
//! pack asks how much effort the reply needs (`effort`: low, medium, high,
//! xhigh, max, or `default`), and this module decides, purely, what the
//! turn's request carries:
//!
//! - **Jev's level, when the answer reached the question's bar, is the effort**
//!   ([`decide_open`]), whatever the level and wherever the model sits: no
//!   bounds, no high-effort gate, no floor, no fixed profile. `default`, or an
//!   answer under the bar, is the model's own default effort.
//! - **Only a model that takes effort** (its catalog row's `effort`): for any
//!   other the answer is recorded and the request carries none.
//! - **Sticky on a model without per-message effort** ([`sticky`], theseus-rwpbg.4):
//!   there a change of the top-level effort restarts the messages' cache, so on a
//!   long conversation the request keeps the previous request's level unless
//!   Jev's differs from it by two steps or more. The decision keeps both: `ran`
//!   is the level Jev's answer came to, `sent` the one the request carries.
//! - **The rest are recorded only**: a verdict in shadow, a pinned turn, and a
//!   late verdict (one asked for the session's last message, which carries its
//!   model, never its effort): the profile's own effort stands.

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
    /// Jev's level.
    Applied,
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
            EffortReason::NoEffort => "no_effort",
            EffortReason::Carried => "carried",
            EffortReason::Recorded => "recorded",
            EffortReason::Sticky => "sticky",
            EffortReason::StickyReverted => "sticky_reverted",
            EffortReason::ModelDefault => "model_default",
        }
    }
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
                | EffortReason::Sticky
                | EffortReason::StickyReverted
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
    /// A held `applied` answer.
    Sticky,
}

/// Whether an answer that disagrees with the held level ends the hold early.
/// Inside a `sticky` window the answer must be `sure` (its band reached the
/// question's own `confirm` bar). No window, no revert.
pub fn reverts(window: Option<Hold>, answer: &EffortAnswer) -> bool {
    match window {
        None => false,
        Some(Hold::Sticky) => answer.sure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(level: &str, confidence: f64, sure: bool) -> EffortAnswer {
        EffortAnswer {
            level: level.into(),
            confidence,
            sure,
        }
    }

    #[test]
    fn every_level_is_a_pack_option() {
        for e in [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
        ] {
            assert_eq!(parse(name(e)), Some(e));
        }
        assert_eq!(parse("unclear"), None);
        assert_eq!(parse("default"), None);
    }

    #[test]
    fn the_wires_doc_lists_every_effort_reason_word() {
        let doc = include_str!("../../../theseus-protocol/src/route.rs");
        let doc = &doc[doc.find("Why the turn ran at its effort").unwrap()..];
        let doc = &doc[..doc.find("pub effort_reason").unwrap()];
        let mut at = 0;
        for r in [
            EffortReason::Applied,
            EffortReason::NoEffort,
            EffortReason::Carried,
            EffortReason::Recorded,
            EffortReason::Sticky,
            EffortReason::StickyReverted,
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

    /// The revert's bar (theseus-u5a37): a `sticky` window needs a sure
    /// answer; no window, none.
    #[test]
    fn a_sticky_hold_reverts_on_its_own_bar() {
        for (window, sure, c, reverts_) in [
            (Some(Hold::Sticky), true, 0.60, true),
            (Some(Hold::Sticky), true, 0.95, true),
            (Some(Hold::Sticky), false, 0.59, false),
            (Some(Hold::Sticky), false, 0.35, false),
            (None, true, 0.95, false),
            (None, false, 0.59, false),
        ] {
            let got = reverts(window, &at("medium", c, sure));
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
}
