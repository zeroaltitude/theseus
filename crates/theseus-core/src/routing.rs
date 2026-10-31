//! Routing (M5 step 25e, route.v10 since theseus-h66v4): Jev chooses the model.
//! The route pack judges, at `inbound`, which model a person's message needs and
//! the effort its reply needs ([`effort`]), and this module decides, purely, which
//! profile the turn runs on: Jev's pick, under the data rule's floor and the cache
//! hold ([`model::decide`], re-exported as [`decide`]). The turn applies it
//! (`turn::route_step`).
//!
//! - **Usable** (model providers have no breaker; only Jev has): configured,
//!   its provider's key settled, its model priced in the catalog, and able to
//!   read the turn's images (glm-5.3 is text only).
//! - **A switch**: the session's routed profile moves, at once while the first
//!   compile's estimate is under `cold_switch_tokens`, above it only when the
//!   turn before agreed (a [`Hold`]): the switch recompiles, strips the prefix's
//!   thinking, and leaves the cache cold.
//! - **A `short` context** (`model::SHORT`): that turn alone runs on the pick;
//!   the session's profile, `last_target` and compilation stay as they were.
//! - **A late verdict** (one that came after its message's wait) applies to
//!   the session's next message alone, and a `short` one's to none ([`carries`]).
//! - **A task's brief** (theseus-ypnxt.7, [`brief::brief_base`]): the verdict
//!   on a task's brief picks the task's base, at once and for good.
//! - **A task's checkpoints** (theseus-ypnxt.9, [`checkpoint`]): a running task
//!   asks the same pack again at a loop boundary, when it keeps failing, stalls,
//!   changes phase or is about to report. Jev's verdict moves it, up or down, by
//!   the cache hold's rule alone.
//! - **No cap**: a place's profile is its default, not a ceiling; no class list,
//!   dear-tier guard, step down or switch limit is read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::CatalogEntry;
use crate::config::RoutingConfig;

pub mod brief;
pub mod checkpoint;
pub mod effort;
pub mod model;
pub mod personal;
#[cfg(test)]
mod tests_effort_open;
#[cfg(test)]
mod tests_model;
pub use personal::PersonalAnswer;

/// A short turn's uncached input, in tokens, for `cheapest` and the cap.
pub const SHORT_INPUT: u64 = 4_000;
/// A short turn's output, in tokens.
pub const SHORT_OUTPUT: u64 = 500;

/// What the session keeps of routing (a stored field: store format 15;
/// its base, `from`, 21).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routed {
    /// The profile routing moved the session to; its turns run there unless
    /// the owner chooses another, while `route.v1` acts live for them. A turn
    /// that finds it not acting, or the live profile switched since the
    /// session's last turn began, clears this (theseus-9yyr).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The session's base, by the profile's name: where it ran before
    /// routing first moved it (theseus-0j2.17). Once `route.v1` stops acting,
    /// its turns run here, so a pane's `-P` profile comes back; a turn whose
    /// base is another (`[model] live` changed and the session follows it,
    /// or its place's profile did) clears the move. Absent in records
    /// written before it (store format 21): the turn's base is taken as it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// A switch the cache held back: the next agreeing turn makes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<Hold>,
}

/// A switch held back above `cold_switch_tokens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    pub mode: String,
    pub profile: String,
    /// The turn that held it.
    pub turn: String,
}

/// Why a turn runs where it runs (the `route.decided` row's `reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The verdict's mode picked the profile (or the session's own, for a
    /// mode with no list, or the one it is on).
    Verdict,
    /// A detour's message (`trivial`, `quick`).
    Detour,
    /// No profile in the mode's list was usable: the session's own.
    Fallback,
    /// A switch the cache held back, waiting for a second agreeing turn.
    CacheHold,
    /// The verdict's confidence was under `switch_confidence`.
    Unsure,
    /// The verdict came after the wait: it applies from the next message.
    Late,
    /// Jev was known unreachable (its last try failed to connect), so the
    /// turn did not wait; a verdict that comes applies as a late one
    /// (theseus-otny).
    Unreachable,
    /// Jev gave no verdict (down, slow past its call, rate-limited,
    /// malformed, the breaker, the budget).
    NoVerdict,
    /// The owner chose the profile, provider, or model: recorded in shadow.
    Pinned,
    /// `route.v1` in shadow: recorded, never routed.
    Shadow,
    /// The owner's correction, or the layer's entry close to the message,
    /// placed the turn ahead of the verdict (theseus-q31l).
    Correction,
    /// The message names a sensitive name and the turn's profile was not of
    /// US origin, so the `[model]` default runs (theseus-oejh3, theseus-xs5ah).
    DataGate,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Verdict => "verdict",
            Reason::Detour => "detour",
            Reason::Fallback => "fallback",
            Reason::CacheHold => "cache_hold",
            Reason::Unsure => "unsure",
            Reason::Late => "late",
            Reason::Unreachable => "unreachable",
            Reason::NoVerdict => "no_verdict",
            Reason::Pinned => "pinned",
            Reason::Shadow => "shadow",
            Reason::Correction => "correction",
            Reason::DataGate => "data_gate",
        }
    }
}

/// A verdict: the route pack's answer, as the turn reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub mode: String,
    pub confidence: f64,
    /// route.v3's answer about the reply's effort (theseus-qe3v), when the
    /// pack asks it and Jev answered it.
    pub effort: Option<effort::EffortAnswer>,
    /// The judgment's id.
    pub judgment: String,
    /// The turn it was asked for.
    pub turn: String,
    /// route.v5's answers (theseus-c1uz): which persona, and whether to seal.
    pub persona: Option<Box<crate::engage::PersonaAnswer>>,
    pub seal: Option<Box<crate::engage::SealAnswer>>,
    /// route.v7's `personal` answer (theseus-gasov): a reach for the
    /// operator's personal details warns the owner.
    pub personal: Option<Box<PersonalAnswer>>,
    /// route.v10's `model` answer (theseus-h66v4): the option Jev chose, and
    /// whether its band reached the question's `confirm` bar. A verdict that
    /// carries one is decided by [`model::decide`], never by the class walk.
    pub model: Option<Box<Pick>>,
    /// route.v10's `context` answer: `short` or `full`.
    pub context: Option<Box<Pick>>,
}

/// One of route.v10's choice answers (`model`, `context`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    /// The option Jev chose.
    pub choice: String,
    pub confidence: f64,
    /// Its band reached the question's `confirm` bar (act or confirm): what
    /// the pack's own bar calls an answer to act on.
    pub sure: bool,
}

impl Verdict {
    /// The verdict is route.v10's: it answers `model` or `context`, and says
    /// no `mode`.
    pub fn open(&self) -> bool {
        self.model.is_some() || self.context.is_some()
    }

    /// Jev said `short` and its band reached the bar: this message alone is
    /// answered from the persona, the last exchanges and the recall (the
    /// detour compile), never from the session's compilation.
    pub fn short(&self) -> bool {
        self.context
            .as_ref()
            .is_some_and(|c| c.choice == model::SHORT && c.sure)
    }
}

/// One configured profile, as routing weighs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub provider: String,
    pub model: String,
    /// Why it cannot run a turn now, if it cannot.
    pub unusable: Option<&'static str>,
    /// A short turn's cost at catalog prices, in dollars (none: unpriced).
    pub short_cost: Option<f64>,
    /// The catalog's input price per million tokens (none: unpriced).
    pub input_per_mtok: Option<f64>,
    pub vision: bool,
    /// Origin `cn` or `unknown`: such a profile runs only for a cleared ask,
    /// and only when a class names it in `cn_profiles` (theseus-a1wvq.4).
    pub restricted: bool,
}

/// What a `turn.submit` names that is the owner's choice: a provider or a
/// model, or a profile the pane did not carry from the last turn.
pub fn chosen(p: &theseus_protocol::TurnSubmitParams) -> Option<String> {
    let profile = p.profile.as_ref().filter(|_| !p.carried);
    match (profile, &p.provider, &p.model) {
        (None, None, None) => None,
        (pr, pv, m) => Some(
            [
                pr.map(|x| format!("profile {x}")),
                pv.as_ref().map(|x| format!("provider {x}")),
                m.as_ref().map(|x| format!("model {x}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", "),
        ),
    }
}

/// What `theseus profile use` pins, as [`chosen`]'s words say it
/// (`live profile haiku`): a pin of the live profile, not of a turn's own.
pub const LIVE_PIN: &str = "live profile ";

/// A pin as `theseus why` words it: `-P fable`, `-p zai`, `-m glm-5.3`,
/// `profile use haiku`, from [`chosen`]'s own words.
pub fn pin_words(chosen: &str) -> String {
    chosen
        .split(", ")
        .map(|part| match part.split_once(' ') {
            _ if part.starts_with(LIVE_PIN) => format!("profile use {}", &part[LIVE_PIN.len()..]),
            Some(("profile", x)) => format!("-P {x}"),
            Some(("provider", x)) => format!("-p {x}"),
            Some(("model", x)) => format!("-m {x}"),
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A short turn's cost at a catalog row's prices, in dollars.
pub fn short_cost(e: &CatalogEntry) -> f64 {
    (SHORT_INPUT as f64 * e.input_per_mtok + SHORT_OUTPUT as f64 * e.output_per_mtok) / 1e6
}

/// Every configured profile, by name, as routing weighs it.
pub type Profiles = BTreeMap<String, Profile>;

impl Profile {
    fn runs(&self, images: bool, cleared: bool) -> bool {
        self.unusable.is_none()
            && self.short_cost.is_some()
            && (self.vision || !images)
            && (!self.restricted || cleared)
    }
}

/// What a turn asks the rule.
#[derive(Debug, Clone)]
pub struct Ask<'a> {
    pub verdict: &'a Verdict,
    /// The profile the turn runs on without routing: the session's.
    pub base: &'a str,
    /// The first compile's estimate.
    pub est_tokens: u64,
    pub hold: Option<&'a Hold>,
    pub images: bool,
    /// An open request can be built for this ask, so a restricted profile may
    /// run (false until the open-request row, theseus-a1wvq.8).
    pub cleared: bool,
}

/// What the hold becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldNext {
    Keep,
    Clear,
    Set(Hold),
}

/// The rule's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The profile the turn runs on.
    pub profile: String,
    pub reason: Reason,
    /// This turn alone (a detour's mode): the session stays where it was.
    pub detour: bool,
    /// The session's routed profile moves to `profile`.
    pub switch: bool,
    pub hold: HoldNext,
}

/// The switch rule: Jev's `model` answer, under the data rule's floor and the
/// cache hold ([`model::decide`]).
pub use model::decide;

/// A correction's place for the turn (theseus-q31l): `profile`, if it runs
/// the turn under the place's cap. The owner's own correction (`at_once`)
/// switches the session whatever the context's size; a layer's entry keeps
/// the cache's rule, held above `cold_switch_tokens` until a second turn
/// agrees.
pub fn steer_to(
    cfg: &RoutingConfig,
    profiles: &Profiles,
    a: &Ask<'_>,
    profile: &str,
    at_once: bool,
) -> Decision {
    let stay = |reason| Decision {
        profile: a.base.to_string(),
        reason,
        detour: false,
        switch: false,
        hold: HoldNext::Clear,
    };
    match profiles.get(profile) {
        Some(p) if !p.runs(a.images, a.cleared) => return stay(Reason::Fallback),
        Some(_) => {}
        None => return stay(Reason::Fallback),
    }
    if profile == a.base {
        return stay(Reason::Correction);
    }
    let agreed = a.hold.is_some_and(|h| h.profile == profile);
    if at_once || a.est_tokens < cfg.cold_switch_tokens || agreed {
        return Decision {
            profile: profile.to_string(),
            reason: Reason::Correction,
            detour: false,
            switch: true,
            hold: HoldNext::Clear,
        };
    }
    Decision {
        hold: HoldNext::Set(Hold {
            mode: "correction".into(),
            profile: profile.to_string(),
            turn: a.verdict.turn.clone(),
        }),
        ..stay(Reason::CacheHold)
    }
}

/// Whether a verdict that came after its own message's wait may apply to the
/// session's next message (theseus-6n5j). A switch outlasts its message, so it
/// carries; a `short` pick is its message's alone ("thanks", "what port is it
/// on?"), so a late one never applies to another message.
pub fn carries(v: &Verdict) -> bool {
    !v.short()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(provider: &str, model: &str, cost: f64, vision: bool) -> Profile {
        Profile {
            provider: provider.into(),
            model: model.into(),
            unusable: None,
            short_cost: Some(cost),
            input_per_mtok: None,
            vision,
            restricted: false,
        }
    }

    /// The template's five, at their catalog costs for a short turn. They stay
    /// `restricted: false`: these fixtures weigh cost order, not origin
    /// (`tests_cn` marks the `zai` ones restricted).
    pub(super) fn five() -> Profiles {
        let c = crate::catalog::Catalog::builtin();
        let mut out = Profiles::new();
        for (name, provider, model) in [
            ("sonnet", "anthropic", "claude-sonnet-5-5"),
            ("glm", "zai", "glm-5.3-flash"),
            ("opus", "anthropic", "claude-opus-5-5"),
            ("fable", "anthropic", "claude-fable-5-1"),
            ("glm53", "zai", "glm-5.3"),
        ] {
            let e = c.get(model).unwrap();
            let priced = Profile {
                input_per_mtok: Some(e.input_per_mtok),
                ..p(provider, model, short_cost(e), e.vision)
            };
            out.insert(name.into(), priced);
        }
        out
    }

    pub(super) fn verdict(mode: &str, confidence: f64) -> Verdict {
        Verdict {
            mode: mode.into(),
            confidence,
            effort: None,
            judgment: "jdg_1".into(),
            turn: "turn_1".into(),
            persona: None,
            seal: None,
            personal: None,
            model: None,
            context: None,
        }
    }

    pub(super) fn ask<'a>(v: &'a Verdict, base: &'a str, est: u64) -> Ask<'a> {
        Ask {
            verdict: v,
            base,
            est_tokens: est,
            hold: None,
            images: false,
            cleared: false,
        }
    }

    /// A late verdict carries to the next message, but a `short` one's never
    /// does: its detour is its own message's alone (theseus-6n5j).
    #[test]
    fn a_late_detour_verdict_never_carries() {
        let short = |context: &str| Verdict {
            context: Some(Box::new(Pick {
                choice: context.into(),
                confidence: 0.99,
                sure: true,
            })),
            ..verdict("", 0.99)
        };
        assert!(!carries(&short(model::SHORT)));
        assert!(carries(&short(model::FULL)));
        assert!(carries(&verdict("", 0.9)), "no context answer at all");
    }

    /// A correction's profile (theseus-q31l): the owner's switches at once,
    /// a layer's waits above `cold_switch_tokens` for a second agreeing turn.
    #[test]
    fn a_correction_switches_at_once_and_a_layers_entry_keeps_the_cache_rule() {
        let cfg = RoutingConfig::default();
        let ps = five();
        let v = verdict("chat", 0.9);
        let d = steer_to(&cfg, &ps, &ask(&v, "sonnet", 200_000), "fable", true);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("fable", Reason::Correction, true)
        );
        let d = steer_to(&cfg, &ps, &ask(&v, "sonnet", 200_000), "fable", false);
        assert_eq!(
            (d.profile.as_str(), d.reason),
            ("sonnet", Reason::CacheHold)
        );
        let HoldNext::Set(hold) = d.hold else {
            panic!("{:?}", d.hold)
        };
        let d = steer_to(
            &cfg,
            &ps,
            &Ask {
                hold: Some(&hold),
                ..ask(&v, "sonnet", 200_000)
            },
            "fable",
            false,
        );
        assert!(d.switch, "the second agreeing turn");
        let d = steer_to(&cfg, &ps, &ask(&v, "sonnet", 10), "fable", false);
        assert!(d.switch, "under cold_switch_tokens");
        let d = steer_to(&cfg, &ps, &ask(&v, "sonnet", 10), "fable", true);
        assert_eq!(
            (d.profile.as_str(), d.reason),
            ("fable", Reason::Correction),
            "no place cap: the owner's word is not capped"
        );
        let d = steer_to(&cfg, &ps, &ask(&v, "sonnet", 10), "nope", true);
        assert_eq!(d.reason, Reason::Fallback);
        let d = steer_to(&cfg, &ps, &ask(&v, "fable", 10), "fable", true);
        assert_eq!((d.reason, d.switch), (Reason::Correction, false));
    }
}
