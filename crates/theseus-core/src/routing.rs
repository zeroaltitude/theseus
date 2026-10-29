//! Routing (M5 step 25e): the model per interaction mode. The route pack
//! (`route.v3` since theseus-qe3v) judges, at `inbound`, which mode a
//! person's message needs (`judge::inbound`), and the effort its reply needs
//! ([`effort`], which decides the effort the turn runs at), and
//! this module decides, purely, which profile the turn runs on: the mode's
//! first usable profile, under the place's cap, by the switch rule. The turn
//! applies it (`turn::route_step`).
//!
//! - **Usable** (model providers have no breaker; only Jev has): configured,
//!   its provider's key settled, its model priced in the catalog, and able to
//!   read the turn's images (glm-5.3 is text only).
//! - **`cheapest`**: the usable profile cheapest for a short turn
//!   ([`SHORT_INPUT`] uncached input tokens and [`SHORT_OUTPUT`] output, at
//!   catalog prices).
//! - **A detour** (`trivial` and `quick`, [`is_detour`]): that turn alone
//!   runs on the mode's profile; the session's profile, `last_target` and
//!   compilation stay as they were.
//! - **A switch** (any other mode): the session's routed profile moves, at
//!   once while the first compile's estimate is under `cold_switch_tokens`,
//!   above it only when the turn before agreed (a [`Hold`]): the switch
//!   recompiles, strips the prefix's thinking, and leaves the cache cold.
//! - **Confidence.** A verdict under its mode's bar routes nothing, a detour
//!   included, and breaks a hold's row. The bar is the mode's own
//!   `switch_confidence`, else trivial's 0.4, else the section's 0.6
//!   (`RoutingConfig::confidence_for`, theseus-6n5j).
//! - **A late verdict** (one that came after its message's wait) applies to
//!   the session's next message alone, and a detour's to none ([`carries`]).
//! - **The dear tier's guard** (theseus-p8dy): at a compile over
//!   `dear_tier_max_tokens`, a `sophisticated` or `review` verdict skips a
//!   profile in the mode's list whose catalog input price is above the next
//!   usable one's, and the turn says `context_guard`.
//! - **A step back down** (theseus-p8dy): only Jev elevates. A session Jev
//!   moved above its base (`Routed.from`) goes back to it once Jev's verdicts
//!   for two turns in a row name a mode whose first profile is the base or
//!   cheaper (`chat`, `routine_coding`, `other`): the first sets a [`Hold`]
//!   named [`STEP_DOWN`] toward the base, the second makes the switch, at any
//!   estimate, and a verdict of another mode, or one under its bar, breaks
//!   the row (a detour leaves it, as for any hold). Never below the base.
//!   An owner's correction (`steer_to`) never steps down.
//! - **A task's brief** (theseus-ypnxt.7, [`brief::brief_base`]): the verdict
//!   on a task's brief picks the task's base, the class's first usable
//!   profile under the cap, at once and for good (a detour's class too).
//! - **A task's checkpoints** (theseus-ypnxt.9, [`checkpoint`]): a running task
//!   asks the same pack again at a loop boundary, when it keeps failing, stalls,
//!   changes phase or is about to report. Only Jev elevates it; it steps back
//!   down to its base after two agreeing verdicts, never below it.
//! - **The cap.** A place's profile is its default and its cap: a profile
//!   dearer than it at catalog prices is passed over, and the turn says
//!   `capped`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::CatalogEntry;
use crate::config::routing::{is_detour, CHEAPEST};
use crate::config::RoutingConfig;

pub mod brief;
pub mod checkpoint;
pub mod effort;
pub mod personal;
#[cfg(test)]
mod tests_cn;
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
    /// The mode's pick was dearer than the place's profile.
    Capped,
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
    /// The context was over `dear_tier_max_tokens`, so a dearer first
    /// profile of the mode's list was skipped for the next (theseus-p8dy).
    ContextGuard,
    /// Two agreeing verdicts moved the session back down to its base
    /// (theseus-p8dy).
    StepDown,
    /// The message names a sensitive name and the turn's profile was not of
    /// US origin, so the `[model]` default runs (theseus-oejh3, theseus-xs5ah).
    DataGate,
    /// A task's checkpoint verdict would move it up, but it has made its
    /// `[routing.task_checkpoints] max_switches` moves (theseus-ypnxt.9).
    SwitchCap,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Verdict => "verdict",
            Reason::Detour => "detour",
            Reason::Capped => "capped",
            Reason::Fallback => "fallback",
            Reason::CacheHold => "cache_hold",
            Reason::Unsure => "unsure",
            Reason::Late => "late",
            Reason::Unreachable => "unreachable",
            Reason::NoVerdict => "no_verdict",
            Reason::Pinned => "pinned",
            Reason::Shadow => "shadow",
            Reason::Correction => "correction",
            Reason::ContextGuard => "context_guard",
            Reason::StepDown => "step_down",
            Reason::DataGate => "data_gate",
            Reason::SwitchCap => "switch_cap",
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

/// What a mode's list gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    /// The list is empty: the session's own.
    Own,
    /// Its first usable profile within the cap.
    Profile(String),
    /// Its first usable profile was dearer than the cap: the next one within
    /// it, or none (the session's own).
    Capped(Option<String>),
    /// None of it is usable: the session's own.
    Fallback,
}

/// The cheapest usable profile for a short turn, within `cap` (dollars). A
/// restricted profile is never the cheapest, cleared or not (`runs` with no
/// clearance): it enters a class only by name (theseus-a1wvq.4).
pub fn cheapest(profiles: &Profiles, images: bool, cap: Option<f64>) -> Option<String> {
    profiles
        .iter()
        .filter(|(_, p)| p.runs(images, false))
        .filter(|(_, p)| within(p, cap))
        .min_by(|a, b| {
            a.1.short_cost
                .partial_cmp(&b.1.short_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(b.0))
        })
        .map(|(n, _)| n.clone())
}

fn within(p: &Profile, cap: Option<f64>) -> bool {
    match (cap, p.short_cost) {
        (Some(cap), Some(c)) => c <= cap + 1e-12,
        _ => true,
    }
}

/// A mode's list, walked: its first usable profile within the cap. A
/// restricted profile is usable only when `cleared`.
pub fn pick(
    list: &[String],
    profiles: &Profiles,
    images: bool,
    cap: Option<f64>,
    cleared: bool,
) -> Picked {
    if list.is_empty() {
        return Picked::Own;
    }
    let mut capped = false;
    for name in list {
        let found = if name == CHEAPEST {
            // `cheapest` within no cap, so a dearer cheapest still says capped.
            cheapest(profiles, images, None)
        } else {
            profiles
                .get(name)
                .filter(|p| p.runs(images, cleared))
                .map(|_| name.clone())
        };
        let Some(found) = found else { continue };
        if within(&profiles[&found], cap) {
            return match capped {
                true => Picked::Capped(Some(found)),
                false => Picked::Profile(found),
            };
        }
        capped = true;
        if name == CHEAPEST {
            if let Some(c) = cheapest(profiles, images, cap) {
                return Picked::Capped(Some(c));
            }
        }
    }
    match capped {
        true => Picked::Capped(None),
        false => Picked::Fallback,
    }
}

/// The modes the dear tier's context guard applies to (theseus-p8dy).
pub const GUARDED: [&str; 2] = ["sophisticated", "review"];

/// A mode's list without the profiles the guard skips: each usable one
/// whose input price is above the next usable one's. None when it skips
/// none. The last usable profile is never skipped.
fn dear_guard(
    list: &[String],
    profiles: &Profiles,
    images: bool,
    cleared: bool,
) -> Option<Vec<String>> {
    let usable: Vec<(usize, &Profile)> = list
        .iter()
        .enumerate()
        .filter_map(|(i, name)| {
            let found = match name == CHEAPEST {
                true => cheapest(profiles, images, None),
                false => Some(name.clone()),
            }?;
            profiles
                .get(&found)
                .filter(|p| p.runs(images, cleared))
                .map(|p| (i, p))
        })
        .collect();
    let skip: Vec<usize> = usable
        .windows(2)
        .filter(|w| {
            matches!((w[0].1.input_per_mtok, w[1].1.input_per_mtok), (Some(a), Some(b)) if a > b)
        })
        .map(|w| w[0].0)
        .collect();
    (!skip.is_empty()).then(|| {
        let keep = |(i, _): &(usize, &String)| !skip.contains(i);
        list.iter()
            .enumerate()
            .filter(keep)
            .map(|(_, n)| n.clone())
            .collect()
    })
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
    /// The session's base when Jev moved it up from there (`Routed.from`
    /// while `Routed.profile` is set); none for an owner's correction, which
    /// is never stepped down.
    pub from: Option<&'a str>,
    pub images: bool,
    /// The place's profile's short-turn cost, when a place caps it.
    pub cap: Option<f64>,
    /// An open request can be built for this ask, so a restricted profile may
    /// run (false until the open-request row, theseus-a1wvq.8).
    pub cleared: bool,
}

/// The `mode` of the [`Hold`] a step back down sets (theseus-p8dy).
pub const STEP_DOWN: &str = "step_down";

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

/// The step back down to the session's base (theseus-p8dy), for a verdict
/// whose mode would pick `picked`: only when Jev moved the session above its
/// base (`a.from`, dearer than it), the mode's pick is the session's own or a
/// profile no dearer than the base, and the base is usable. The first such
/// turn holds toward the base; the second, which finds that hold, switches.
fn step_down(profiles: &Profiles, a: &Ask<'_>, picked: &Picked) -> Option<Decision> {
    let from = a.from.filter(|f| *f != a.base)?;
    let base = profiles.get(from).filter(|p| p.runs(a.images, a.cleared))?;
    let (cur, base_cost) = (profiles.get(a.base)?.short_cost?, base.short_cost?);
    if cur <= base_cost + 1e-12 {
        return None;
    }
    let eligible = match picked {
        Picked::Own => true,
        Picked::Profile(p) | Picked::Capped(Some(p)) => profiles
            .get(p)
            .and_then(|p| p.short_cost)
            .is_some_and(|c| c <= base_cost + 1e-12),
        Picked::Capped(None) | Picked::Fallback => false,
    };
    if !eligible {
        return None;
    }
    let held = a
        .hold
        .is_some_and(|h| h.mode == STEP_DOWN && h.profile == from);
    Some(match held {
        true => Decision {
            profile: from.to_string(),
            reason: Reason::StepDown,
            detour: false,
            switch: true,
            hold: HoldNext::Clear,
        },
        false => Decision {
            profile: a.base.to_string(),
            reason: Reason::CacheHold,
            detour: false,
            switch: false,
            hold: HoldNext::Set(Hold {
                mode: STEP_DOWN.into(),
                profile: from.to_string(),
                turn: a.verdict.turn.clone(),
            }),
        },
    })
}

/// The switch rule.
pub fn decide(cfg: &RoutingConfig, profiles: &Profiles, a: &Ask<'_>) -> Decision {
    let detour = is_detour(&a.verdict.mode);
    let stay = |reason, hold| Decision {
        profile: a.base.to_string(),
        reason,
        detour: false,
        switch: false,
        hold,
    };
    let keep_or_clear = if detour {
        HoldNext::Keep
    } else {
        HoldNext::Clear
    };
    if a.verdict.confidence < cfg.confidence_for(&a.verdict.mode) {
        return stay(Reason::Unsure, keep_or_clear);
    }
    let list = cfg.classes.effective(&a.verdict.mode, a.cleared);
    let kept = (GUARDED.contains(&a.verdict.mode.as_str())
        && a.est_tokens > cfg.dear_tier_max_tokens)
        .then(|| dear_guard(&list, profiles, a.images, a.cleared))
        .flatten();
    let picked = pick(&list, profiles, a.images, a.cap, a.cleared);
    let guarded = kept
        .map(|k| pick(&k, profiles, a.images, a.cap, a.cleared))
        .filter(|g| *g != picked);
    let (picked, guard) = match guarded {
        Some(g) => (g, true),
        None => (picked, false),
    };
    let stepped = match detour {
        true => None,
        false => step_down(profiles, a, &picked),
    };
    if let Some(d) = stepped {
        return d;
    }
    let (target, reason) = match picked {
        Picked::Own => (a.base.to_string(), Reason::Verdict),
        Picked::Profile(p) if guard => (p, Reason::ContextGuard),
        Picked::Profile(p) => (p, Reason::Verdict),
        Picked::Capped(Some(p)) => (p, Reason::Capped),
        Picked::Capped(None) => (a.base.to_string(), Reason::Capped),
        Picked::Fallback => (a.base.to_string(), Reason::Fallback),
    };
    if target == a.base {
        return stay(reason, keep_or_clear);
    }
    if detour {
        return Decision {
            profile: target,
            reason: match reason {
                Reason::Capped => Reason::Capped,
                _ => Reason::Detour,
            },
            detour: true,
            switch: false,
            hold: HoldNext::Keep,
        };
    }
    let agreed = a.hold.is_some_and(|h| h.profile == target);
    if a.est_tokens < cfg.cold_switch_tokens || agreed {
        return Decision {
            profile: target,
            reason,
            detour: false,
            switch: true,
            hold: HoldNext::Clear,
        };
    }
    stay(
        Reason::CacheHold,
        HoldNext::Set(Hold {
            mode: a.verdict.mode.clone(),
            profile: target,
            turn: a.verdict.turn.clone(),
        }),
    )
}

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
        Some(p) if !within(p, a.cap) => return stay(Reason::Capped),
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
/// session's next message (theseus-6n5j). A switch's mode is the
/// conversation's, and the switch it makes outlasts its message anyway, so it
/// carries; a detour's mode is its message's alone ("thanks", "what port is
/// it on?"), and its detour is that turn's alone, so a late verdict of
/// `trivial` or `quick` never applies to another message (theseus-3okf).
pub fn carries(v: &Verdict) -> bool {
    !is_detour(&v.mode)
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
        }
    }

    pub(super) fn ask<'a>(v: &'a Verdict, base: &'a str, est: u64) -> Ask<'a> {
        Ask {
            verdict: v,
            base,
            est_tokens: est,
            hold: None,
            from: None,
            images: false,
            cap: None,
            cleared: false,
        }
    }

    #[test]
    fn each_modes_first_usable_profile_then_the_next_then_the_sessions() {
        let cfg = RoutingConfig::default();
        let mut ps = five();
        let list = |m: &str| cfg.classes.of(m).to_vec();
        assert_eq!(
            pick(&list("sophisticated"), &ps, false, None, false),
            Picked::Profile("opus".into())
        );
        assert_eq!(
            pick(&list("deep_coding"), &ps, false, None, false),
            Picked::Profile("opus".into())
        );
        // routine_coding's `haikuhi` is not one of the five: `sonnet`.
        assert_eq!(
            pick(&list("routine_coding"), &ps, false, None, false),
            Picked::Profile("sonnet".into())
        );
        assert_eq!(pick(&list("chat"), &ps, false, None, false), Picked::Own);
        assert_eq!(pick(&list("other"), &ps, false, None, false), Picked::Own);
        // The first unusable: the next. `sophisticated` is `opushi`, `opus`,
        // `sonnethi` (the cn rubric's §7.4): of these five only `opus`
        // exists, so without it the turn stays on the session's own.
        ps.get_mut("opus").unwrap().unusable = Some("key");
        assert_eq!(
            pick(&list("sophisticated"), &ps, false, None, false),
            Picked::Fallback
        );
        assert_eq!(
            pick(&list("deep_coding"), &ps, false, None, false),
            Picked::Profile("sonnet".into())
        );
        // An image: glm-5.3 reads none, so glm (5.3 Flash) takes it.
        assert_eq!(
            pick(&["glm53".into(), "glm".into()], &ps, true, None, false),
            Picked::Profile("glm".into())
        );
        // None usable: the session's own.
        ps.get_mut("fable").unwrap().short_cost = None;
        assert_eq!(
            pick(&list("sophisticated"), &ps, false, None, false),
            Picked::Fallback
        );
        let d = decide(
            &cfg,
            &ps,
            &ask(&verdict("sophisticated", 0.95), "sonnet", 100),
        );
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("sonnet", Reason::Fallback, false)
        );
        // Unconfigured names are passed over.
        assert_eq!(
            pick(&["nope".into(), "glm".into()], &ps, false, None, false),
            Picked::Profile("glm".into())
        );
    }

    #[test]
    fn cheapest_is_the_usable_profile_cheapest_for_a_short_turn() {
        let mut ps = five();
        assert_eq!(cheapest(&ps, false, None).as_deref(), Some("glm"));
        ps.get_mut("glm").unwrap().unusable = Some("key");
        assert_eq!(cheapest(&ps, false, None).as_deref(), Some("glm53"));
        // glm-5.3 reads no image.
        assert_eq!(cheapest(&ps, true, None).as_deref(), Some("sonnet"));
        let cfg = RoutingConfig::default();
        assert_eq!(
            pick(cfg.classes.of("trivial"), &ps, false, None, false),
            Picked::Profile("glm53".into())
        );
    }

    #[test]
    fn a_trivial_message_detours_and_holds_nothing() {
        let cfg = RoutingConfig::default();
        let ps = five();
        let v = verdict("trivial", 0.9);
        let d = decide(&cfg, &ps, &ask(&v, "opus", 200_000));
        assert_eq!(
            d,
            Decision {
                profile: "glm".into(),
                reason: Reason::Detour,
                detour: true,
                switch: false,
                hold: HoldNext::Keep
            }
        );
        // A detour needs the confidence too: trivial's own bar, 0.4.
        let d = decide(&cfg, &ps, &ask(&verdict("trivial", 0.39), "opus", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.detour),
            ("opus", Reason::Unsure, false)
        );
    }

    /// The greeting of 2026-10-04 23:45 (theseus-6n5j): trivial at 0.45
    /// stayed on Sonnet under the section's 0.6. Trivial's bar is 0.4, so it
    /// detours now; the switch modes keep 0.6, and a table's own bar wins.
    #[test]
    fn a_trivial_verdict_routes_at_its_own_bar() {
        let cfg = RoutingConfig::default();
        let ps = five();
        let d = decide(&cfg, &ps, &ask(&verdict("trivial", 0.45), "sonnet", 87_000));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.detour, d.switch),
            ("glm", Reason::Detour, true, false)
        );
        let d = decide(&cfg, &ps, &ask(&verdict("trivial", 0.4), "sonnet", 10));
        assert!(d.detour, "at the bar");
        let d = decide(&cfg, &ps, &ask(&verdict("trivial", 0.399), "sonnet", 10));
        assert_eq!((d.reason, d.detour), (Reason::Unsure, false));
        for m in ["sophisticated", "deep_coding", "routine_coding"] {
            let d = decide(&cfg, &ps, &ask(&verdict(m, 0.45), "sonnet", 10));
            assert_eq!((d.reason, d.switch), (Reason::Unsure, false), "{m}");
        }
        let mut own = RoutingConfig::default();
        own.classes.trivial.switch_confidence = Some(0.5);
        own.classes.sophisticated.switch_confidence = Some(0.4);
        let d = decide(&own, &ps, &ask(&verdict("trivial", 0.45), "sonnet", 10));
        assert_eq!(d.reason, Reason::Unsure, "trivial's own bar, raised");
        let d = decide(
            &own,
            &ps,
            &ask(&verdict("sophisticated", 0.45), "sonnet", 10),
        );
        assert_eq!(
            (d.profile.as_str(), d.switch),
            ("opus", true),
            "a switch's own bar"
        );
    }

    /// The five with the template's Haiku 5.5 profiles (theseus-3okf).
    pub(super) fn seven() -> Profiles {
        let c = crate::catalog::Catalog::builtin();
        let e = c.get("claude-haiku-5-5").unwrap();
        let mut out = five();
        for name in ["haiku", "haikuhi"] {
            let h = Profile {
                input_per_mtok: Some(e.input_per_mtok),
                ..p("anthropic", "claude-haiku-5-5", short_cost(e), e.vision)
            };
            out.insert(name.into(), h);
        }
        out
    }

    /// route.v2's placements (theseus-3okf): a trivial and a quick message
    /// each detour to `haiku` for their turn alone, holding nothing, at any
    /// estimate; routine programming switches the session to `haikuhi`; a
    /// config without the Haiku profiles keeps the old way: trivial to the
    /// cheapest, quick on the session's own, routine programming on Sonnet.
    #[test]
    fn trivial_and_quick_detour_to_haiku_and_routine_coding_switches_to_haikuhi() {
        let cfg = RoutingConfig::default();
        let ps = seven();
        for m in ["trivial", "quick"] {
            let d = decide(&cfg, &ps, &ask(&verdict(m, 0.9), "sonnet", 200_000));
            assert_eq!(
                d,
                Decision {
                    profile: "haiku".into(),
                    reason: Reason::Detour,
                    detour: true,
                    switch: false,
                    hold: HoldNext::Keep
                },
                "{m}"
            );
        }
        // quick's bar is the section's 0.6, not trivial's 0.4.
        let d = decide(&cfg, &ps, &ask(&verdict("quick", 0.5), "sonnet", 10));
        assert_eq!((d.reason, d.detour), (Reason::Unsure, false));
        let d = decide(
            &cfg,
            &ps,
            &ask(&verdict("routine_coding", 0.9), "sonnet", 10),
        );
        assert_eq!(
            (d.profile.as_str(), d.reason, d.detour, d.switch),
            ("haikuhi", Reason::Verdict, false, true)
        );
        // Without them.
        let ps = five();
        let d = decide(&cfg, &ps, &ask(&verdict("trivial", 0.9), "sonnet", 10));
        assert_eq!((d.profile.as_str(), d.detour), ("glm", true));
        let mut no_glm = five();
        no_glm.remove("glm");
        let d = decide(&cfg, &no_glm, &ask(&verdict("trivial", 0.9), "sonnet", 10));
        assert_eq!((d.profile.as_str(), d.detour), ("glm53", true), "cheapest");
        let d = decide(&cfg, &ps, &ask(&verdict("quick", 0.9), "sonnet", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.detour, d.switch),
            ("sonnet", Reason::Fallback, false, false)
        );
        let d = decide(&cfg, &ps, &ask(&verdict("routine_coding", 0.9), "opus", 10));
        assert_eq!((d.profile.as_str(), d.switch), ("sonnet", true));
    }

    /// route.v9's two kinds (theseus-gezlj): a `run_tool` and a `commentary`
    /// message each detour to `haiku` for their turn alone, holding nothing, at
    /// any estimate, so a command run never moves the session's profile nor
    /// leaves its cache cold; each takes the section's 0.6 bar (the owner's
    /// words set no other). A config without Haiku runs them on the session's
    /// own profile, as `quick` does.
    #[test]
    fn run_tool_and_commentary_detour_to_haiku_and_hold_nothing() {
        let cfg = RoutingConfig::default();
        let ps = seven();
        for m in ["run_tool", "commentary"] {
            let d = decide(&cfg, &ps, &ask(&verdict(m, 0.9), "sonnet", 200_000));
            assert_eq!(
                d,
                Decision {
                    profile: "haiku".into(),
                    reason: Reason::Detour,
                    detour: true,
                    switch: false,
                    hold: HoldNext::Keep
                },
                "{m}"
            );
            let d = decide(&cfg, &ps, &ask(&verdict(m, 0.5), "sonnet", 10));
            assert_eq!((d.reason, d.detour), (Reason::Unsure, false), "{m}: 0.6");
            // Without Haiku, `run_tool`'s second profile is Sonnet (the cn
            // rubric's §7.4): the session already on it stays; one on
            // another profile runs that turn on Sonnet. `commentary` names
            // Haiku alone, so it stays on the session's own.
            let d = decide(&cfg, &five(), &ask(&verdict(m, 0.9), "sonnet", 10));
            assert_eq!(
                (d.profile.as_str(), d.reason, d.detour, d.switch),
                (
                    "sonnet",
                    if m == "run_tool" {
                        Reason::Verdict
                    } else {
                        Reason::Fallback
                    },
                    false,
                    false
                ),
                "{m}: without Haiku, on Sonnet"
            );
            let d = decide(&cfg, &five(), &ask(&verdict(m, 0.9), "fable", 10));
            assert_eq!(
                (d.profile.as_str(), d.reason, d.detour, d.switch),
                match m {
                    "run_tool" => ("sonnet", Reason::Detour, true, false),
                    _ => ("fable", Reason::Fallback, false, false),
                },
                "{m}: without Haiku, on Fable"
            );
        }
        // A class table of its own takes its own bar and list.
        let mut own = RoutingConfig::default();
        own.classes.run_tool.switch_confidence = Some(0.3);
        own.classes.run_tool.profiles = vec!["haikuhi".into()];
        let d = decide(&own, &ps, &ask(&verdict("run_tool", 0.35), "sonnet", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.detour),
            ("haikuhi", Reason::Detour, true)
        );
    }

    /// A late verdict carries to the next message, but a detour's never
    /// does: its detour is its own message's alone (theseus-6n5j; `quick`
    /// since theseus-3okf; `run_tool` and `commentary` since theseus-gezlj).
    #[test]
    fn a_late_detour_verdict_never_carries() {
        assert!(!carries(&verdict("trivial", 0.99)));
        assert!(!carries(&verdict("quick", 0.99)));
        assert!(!carries(&verdict("run_tool", 0.99)));
        assert!(!carries(&verdict("commentary", 0.99)));
        for m in [
            "chat",
            "sophisticated",
            "deep_coding",
            "routine_coding",
            "other",
        ] {
            assert!(carries(&verdict(m, 0.9)), "{m}");
        }
    }

    #[test]
    fn under_cold_switch_tokens_a_switch_is_at_once_and_above_it_waits_for_agreement() {
        let cfg = RoutingConfig::default();
        let ps = seven();
        let v = verdict("sophisticated", 0.8);
        let d = decide(&cfg, &ps, &ask(&v, "sonnet", 29_999));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("opus", Reason::Verdict, true)
        );
        let d = decide(&cfg, &ps, &ask(&v, "sonnet", 30_000));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("sonnet", Reason::CacheHold, false)
        );
        let HoldNext::Set(hold) = d.hold else {
            panic!("{:?}", d.hold)
        };
        assert_eq!(hold.profile, "opus");
        // The next turn agrees: the switch.
        let v2 = verdict("sophisticated", 0.8);
        let d = decide(
            &cfg,
            &ps,
            &Ask {
                hold: Some(&hold),
                ..ask(&v2, "sonnet", 90_000)
            },
        );
        assert_eq!(
            (d.profile.as_str(), d.switch, &d.hold),
            ("opus", true, &HoldNext::Clear)
        );
        // A turn that disagrees clears it.
        let v3 = verdict("routine_coding", 0.8);
        let d = decide(
            &cfg,
            &ps,
            &Ask {
                hold: Some(&hold),
                ..ask(&v3, "sonnet", 90_000)
            },
        );
        assert_eq!(d.reason, Reason::CacheHold);
        assert!(matches!(d.hold, HoldNext::Set(Hold { ref profile, .. }) if profile == "haikuhi"));
    }

    /// A correction's profile (theseus-q31l): the owner's switches at once,
    /// a layer's waits above `cold_switch_tokens` for a second agreeing turn,
    /// and neither climbs over a place's cap.
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
        let cap = Some(ps["sonnet"].short_cost.unwrap());
        let d = steer_to(
            &cfg,
            &ps,
            &Ask {
                cap,
                ..ask(&v, "sonnet", 10)
            },
            "fable",
            true,
        );
        assert_eq!((d.profile.as_str(), d.reason), ("sonnet", Reason::Capped));
        let d = steer_to(&cfg, &ps, &ask(&v, "sonnet", 10), "nope", true);
        assert_eq!(d.reason, Reason::Fallback);
        let d = steer_to(&cfg, &ps, &ask(&v, "fable", 10), "fable", true);
        assert_eq!((d.reason, d.switch), (Reason::Correction, false));
    }

    #[test]
    fn nothing_routes_under_switch_confidence() {
        let cfg = RoutingConfig::default();
        let ps = five();
        for c in [0.0, 0.3, 0.599] {
            let d = decide(&cfg, &ps, &ask(&verdict("sophisticated", c), "sonnet", 10));
            assert_eq!(
                (d.profile.as_str(), d.reason, d.switch),
                ("sonnet", Reason::Unsure, false)
            );
            assert_eq!(d.hold, HoldNext::Clear, "an unsure turn breaks the row");
        }
        let d = decide(
            &cfg,
            &ps,
            &ask(&verdict("sophisticated", 0.6), "sonnet", 10),
        );
        assert!(d.switch);
    }

    #[test]
    fn a_places_cap_lets_a_detour_below_it_and_says_capped_above_it() {
        let cfg = RoutingConfig::default();
        let ps = five();
        let cap = Some(ps["sonnet"].short_cost.unwrap());
        // Below the cap: the detour goes.
        let v = verdict("trivial", 0.9);
        let d = decide(
            &cfg,
            &ps,
            &Ask {
                cap,
                ..ask(&v, "sonnet", 10)
            },
        );
        assert_eq!(
            (d.profile.as_str(), d.reason, d.detour),
            ("glm", Reason::Detour, true)
        );
        // Opus and Fable are dearer than Sonnet: nothing climbs above it.
        let v = verdict("sophisticated", 0.95);
        let d = decide(
            &cfg,
            &ps,
            &Ask {
                cap,
                ..ask(&v, "sonnet", 10)
            },
        );
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("sonnet", Reason::Capped, false)
        );
        // deep_coding's opus is dearer, its sonnet is the place's own.
        let v = verdict("deep_coding", 0.95);
        let d = decide(
            &cfg,
            &ps,
            &Ask {
                cap,
                ..ask(&v, "sonnet", 10)
            },
        );
        assert_eq!((d.profile.as_str(), d.reason), ("sonnet", Reason::Capped));
        // Under a glm cap, routine coding's haikuhi is cheaper and goes;
        // without it, its sonnet is dearer: capped, the session's own.
        let mut ps = seven();
        let cap = Some(ps["glm"].short_cost.unwrap());
        let v = verdict("routine_coding", 0.95);
        let at = |ps: &Profiles| {
            decide(
                &cfg,
                ps,
                &Ask {
                    cap,
                    ..ask(&v, "opus", 10)
                },
            )
        };
        let d = at(&ps);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("haikuhi", Reason::Verdict, true)
        );
        ps.get_mut("haikuhi").unwrap().unusable = Some("key");
        let d = at(&ps);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("opus", Reason::Capped, false)
        );
    }

    /// The dear tier's context guard (theseus-p8dy): over
    /// `dear_tier_max_tokens`, a `sophisticated` verdict skips a first
    /// profile whose input price is above the next usable one's; at the
    /// threshold and under it nothing changes; nothing is skipped when the
    /// first is the cheaper, and the last usable profile is never skipped.
    #[test]
    fn over_the_dear_tiers_threshold_a_dearer_first_profile_is_skipped() {
        // Every switch is at once, so the cache rule is out of the way.
        let mut cfg = RoutingConfig {
            cold_switch_tokens: u64::MAX,
            ..RoutingConfig::default()
        };
        let mut ps = five();
        let (dear, cheap) = ("opus", "fable");
        ps.get_mut(dear).unwrap().input_per_mtok = Some(10.0);
        ps.get_mut(cheap).unwrap().input_per_mtok = Some(5.0);
        cfg.classes.sophisticated.profiles = vec![dear.into(), cheap.into()];
        let v = verdict("sophisticated", 0.9);
        let at =
            |est: u64, ps: &Profiles, cfg: &RoutingConfig| decide(cfg, ps, &ask(&v, "sonnet", est));
        let d = at(200_000, &ps, &cfg);
        assert_eq!(
            (d.profile.as_str(), d.reason),
            (dear, Reason::Verdict),
            "at the threshold, not over it"
        );
        let d = at(200_001, &ps, &cfg);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            (cheap, Reason::ContextGuard, true),
            "over it"
        );
        // The threshold is the config's.
        cfg.dear_tier_max_tokens = 300_000;
        assert_eq!(at(250_000, &ps, &cfg).profile, dear);
        cfg.dear_tier_max_tokens = 200_000;
        // Cheaper first: nothing skipped.
        cfg.classes.sophisticated.profiles = vec![cheap.into(), dear.into()];
        let d = at(300_000, &ps, &cfg);
        assert_eq!((d.profile.as_str(), d.reason), (cheap, Reason::Verdict));
        // The cheaper unusable: the dear one is the last usable, kept.
        cfg.classes.sophisticated.profiles = vec![dear.into(), cheap.into()];
        ps.get_mut(cheap).unwrap().unusable = Some("key");
        let d = at(300_000, &ps, &cfg);
        assert_eq!((d.profile.as_str(), d.reason), (dear, Reason::Verdict));
        // Another mode is not guarded.
        let v2 = verdict("deep_coding", 0.9);
        let d = decide(&cfg, &five(), &ask(&v2, "sonnet", 400_000));
        assert_eq!((d.profile.as_str(), d.reason), ("opus", Reason::Verdict));
        assert_eq!(Reason::ContextGuard.as_str(), "context_guard");
    }

    /// The reason is the guard's only where it moved the pick.
    #[test]
    fn the_guard_says_so_only_where_it_moved_the_pick() {
        let mut cfg = RoutingConfig {
            cold_switch_tokens: u64::MAX,
            ..RoutingConfig::default()
        };
        let mut ps = five();
        ps.get_mut("opus").unwrap().input_per_mtok = Some(10.0);
        ps.get_mut("fable").unwrap().input_per_mtok = Some(5.0);
        cfg.classes.sophisticated.profiles = vec!["fable".into(), "opus".into()];
        let v = verdict("sophisticated", 0.9);
        let d = decide(&cfg, &ps, &ask(&v, "sonnet", 500_000));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("fable", Reason::Verdict, true),
            "the first was already the cheaper"
        );
        cfg.classes.sophisticated.profiles = vec!["opus".into(), "fable".into()];
        let d = decide(&cfg, &ps, &ask(&v, "sonnet", 500_000));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("fable", Reason::ContextGuard, true)
        );
    }

    /// The step back down (theseus-p8dy): a session Jev moved up from its
    /// base goes back after two verdicts in a row whose mode's first profile
    /// is the base or cheaper; one is not enough, a verdict of a dearer mode
    /// or under its bar breaks the row, a detour leaves it, and it never goes
    /// below the base. The hold governs at any size of context.
    #[test]
    fn a_session_jev_moved_up_steps_back_down_after_two_agreeing_turns() {
        let cfg = RoutingConfig::default();
        let ps = seven();
        let chat = verdict("chat", 0.9);
        fn up<'a>(v: &'a Verdict, est: u64, hold: Option<&'a Hold>) -> Ask<'a> {
            Ask {
                from: Some("sonnet"),
                hold,
                ..ask(v, "opus", est)
            }
        }
        // The first turn holds toward the base and stays where it is.
        let d = decide(&cfg, &ps, &up(&chat, 10, None));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("opus", Reason::CacheHold, false)
        );
        let HoldNext::Set(h) = d.hold else {
            panic!("{:?}", d.hold)
        };
        assert_eq!((h.mode.as_str(), h.profile.as_str()), (STEP_DOWN, "sonnet"));
        // The second makes the switch, small context or large.
        for est in [10, 29_999, 30_000, 900_000] {
            let d = decide(&cfg, &ps, &up(&chat, est, Some(&h)));
            assert_eq!(
                (d.profile.as_str(), d.reason, d.switch, &d.hold),
                ("sonnet", Reason::StepDown, true, &HoldNext::Clear),
                "{est}"
            );
        }
        // routine_coding's first profile is dearer-or-equal never: it is
        // cheaper than the base, so it counts, and goes to the base, never
        // below it.
        let rc = verdict("routine_coding", 0.9);
        let d = decide(&cfg, &ps, &up(&rc, 10, Some(&h)));
        assert_eq!((d.profile.as_str(), d.reason), ("sonnet", Reason::StepDown));
        let d = decide(&cfg, &ps, &up(&rc, 10, None));
        assert_eq!((d.reason, d.switch), (Reason::CacheHold, false));
        // A dearer mode breaks the row: the hold clears, and the next chat
        // holds again instead of switching.
        let hard = verdict("sophisticated", 0.9);
        let d = decide(&cfg, &ps, &up(&hard, 10, Some(&h)));
        assert_eq!(
            (d.profile.as_str(), d.reason, &d.hold),
            ("opus", Reason::Verdict, &HoldNext::Clear)
        );
        // So does a verdict under its bar.
        let unsure = verdict("chat", 0.5);
        let d = decide(&cfg, &ps, &up(&unsure, 10, Some(&h)));
        assert_eq!((d.reason, &d.hold), (Reason::Unsure, &HoldNext::Clear));
        // A detour leaves the row as it is.
        let tr = verdict("trivial", 0.9);
        let d = decide(&cfg, &ps, &up(&tr, 10, Some(&h)));
        assert_eq!((d.detour, &d.hold), (true, &HoldNext::Keep));
        // deep_coding leads with opus, which is not the base or cheaper.
        let dc = verdict("deep_coding", 0.9);
        let d = decide(&cfg, &ps, &up(&dc, 10, None));
        assert_eq!((d.profile.as_str(), d.reason), ("opus", Reason::Verdict));
    }

    /// Nothing steps down that Jev did not raise, or that is already at or
    /// below the base: no `from`, the session on its base, a cheaper profile,
    /// an unusable base, and the owner's correction (`steer_to`).
    #[test]
    fn nothing_steps_down_that_jev_did_not_raise() {
        let cfg = RoutingConfig::default();
        let mut ps = seven();
        let chat = verdict("chat", 0.9);
        let d = decide(&cfg, &ps, &ask(&chat, "opus", 10));
        assert_eq!(
            (d.reason, d.hold),
            (Reason::Verdict, HoldNext::Clear),
            "no base known"
        );
        let on_base = Ask {
            from: Some("opus"),
            ..ask(&chat, "opus", 10)
        };
        assert_eq!(decide(&cfg, &ps, &on_base).reason, Reason::Verdict);
        let below = Ask {
            from: Some("opus"),
            ..ask(&chat, "haikuhi", 10)
        };
        let d = decide(&cfg, &ps, &below);
        assert_eq!((d.profile.as_str(), d.reason), ("haikuhi", Reason::Verdict));
        let up = Ask {
            from: Some("sonnet"),
            ..ask(&chat, "opus", 10)
        };
        ps.get_mut("sonnet").unwrap().unusable = Some("key");
        let d = decide(&cfg, &ps, &up);
        assert_eq!((d.profile.as_str(), d.reason), ("opus", Reason::Verdict));
        // The owner's pick or correction, at once, is the owner's.
        let ps = seven();
        let d = steer_to(&cfg, &ps, &Ask { from: None, ..up }, "opus", true);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("opus", Reason::Correction, false)
        );
        let d = steer_to(
            &cfg,
            &ps,
            &Ask {
                from: Some("sonnet"),
                ..ask(&chat, "sonnet", 10)
            },
            "opus",
            true,
        );
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("opus", Reason::Correction, true)
        );
    }

    /// route.v4's `review` (theseus-p8dy, its table moved by theseus-tqomi): a
    /// verification switches the session, at the section's bar, to `opus`,
    /// then `sonnethi` when `opus` is unusable, then the session's own; it is
    /// no detour and its late verdict carries.
    #[test]
    fn a_review_verdict_routes_to_opus_then_sonnethi() {
        let cfg = RoutingConfig::default();
        assert_eq!(cfg.classes.of("review"), ["opus", "sonnethi"]);
        let mut ps = five();
        let c = crate::catalog::Catalog::builtin();
        let e = c.get("claude-sonnet-5-5").unwrap();
        ps.insert(
            "sonnethi".into(),
            p("anthropic", "claude-sonnet-5-5", short_cost(e), e.vision),
        );
        let v = verdict("review", 0.9);
        let d = decide(&cfg, &ps, &ask(&v, "sonnet", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch, d.detour),
            ("opus", Reason::Verdict, true, false)
        );
        ps.get_mut("opus").unwrap().unusable = Some("key");
        let d = decide(&cfg, &ps, &ask(&v, "fable", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("sonnethi", Reason::Verdict, true)
        );
        ps.get_mut("sonnethi").unwrap().unusable = Some("key");
        let d = decide(&cfg, &ps, &ask(&v, "sonnet", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("sonnet", Reason::Fallback, false)
        );
        // Its own bar, the section's 0.6, and a late verdict of it carries.
        let d = decide(&cfg, &five(), &ask(&verdict("review", 0.59), "sonnet", 10));
        assert_eq!(d.reason, Reason::Unsure);
        assert!(carries(&v));
    }
}
