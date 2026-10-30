//! A task's base from its brief (theseus-ypnxt.7). A task starts with no
//! message of its own, so the verdict the route pack gives its brief picks the model
//! it runs on: the class's first usable profile under the place's cap, by
//! [`decide`]'s rule with nothing to keep warm and nothing held.
//!
//! The pick is the task's base, not a move from its parent's model: it is
//! kept as the session's target and routing's `from` is left empty, so only
//! Jev elevates the task from it and a step back down never goes below it.
//! A detour's class (`trivial`, `quick`) is a base here too: a task has no
//! "next message" to leave the model for.
//!
//! A `routine_coding` brief starts on Haiku at high effort ([`CODING_BASE`],
//! `haikuhi`), and its checkpoints move it up when it fails or stalls
//! (theseus-ypnxt.9; the owner, 2026-10-18: start cheap, escalate). A `trivial`
//! brief runs on Haiku.

use super::{decide, Ask, Decision, HoldNext, Profiles, Reason};
use crate::config::RoutingConfig;

/// The profile a `routine_coding` brief tries first, before the class's own
/// list (a config without it falls through to that list).
pub const CODING_BASE: &str = "haikuhi";

/// The rule's answer for the brief `a.verdict` judged, `a.base` being the
/// model the task would otherwise run on (its parent's). `switch` says the
/// task's base is another profile. `open` (the acting route pack asks
/// `model`, theseus-ej6bj): nothing is fixed to a profile, so a
/// `routine_coding` brief starts where the class sends it; closed, it starts
/// on [`CODING_BASE`] as it did (until the row that retires the class rule).
pub fn brief_base(cfg: &RoutingConfig, profiles: &Profiles, a: &Ask<'_>, open: bool) -> Decision {
    let fresh = Ask {
        est_tokens: 0,
        hold: None,
        from: None,
        ..a.clone()
    };
    let mut d = match fresh.verdict.mode.as_str() {
        "routine_coding" if !open => {
            let mut c = cfg.clone();
            c.classes
                .routine_coding
                .profiles
                .retain(|p| p != CODING_BASE);
            c.classes
                .routine_coding
                .profiles
                .insert(0, CODING_BASE.to_string());
            decide(&c, profiles, &fresh)
        }
        _ => decide(cfg, profiles, &fresh),
    };
    if d.detour {
        let reason = match d.reason {
            Reason::Capped => Reason::Capped,
            _ => Reason::Verdict,
        };
        d = Decision {
            detour: false,
            switch: true,
            reason,
            hold: HoldNext::Clear,
            ..d
        };
    }
    d
}

#[cfg(test)]
mod tests {
    use super::super::tests::{ask, five, verdict};
    use super::super::{short_cost, Profile};
    use super::*;

    fn base(v: &str, conf: f64, from: &str, cap: Option<f64>) -> Decision {
        let (cfg, ps, v) = (RoutingConfig::default(), five(), verdict(v, conf));
        brief_base(
            &cfg,
            &ps,
            &Ask {
                cap,
                ..ask(&v, from, 1_000_000)
            },
            false,
        )
    }

    /// A brief judged routine coding from a Fable conversation starts on the
    /// class's first usable profile, whatever the cold-switch size says.
    #[test]
    fn a_routine_brief_from_fable_starts_on_the_classs_profile() {
        let d = base("routine_coding", 0.9, "fable", None);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch, d.detour),
            ("sonnet", Reason::Verdict, true, false)
        );
    }

    /// A routine-coding brief starts on `haikuhi` even when a config moved it
    /// aside, and its checkpoints move it up (theseus-31pgz, theseus-ypnxt.9);
    /// a trivial one runs on Haiku.
    #[test]
    fn a_routine_coding_brief_runs_on_sonnet_and_only_a_trivial_one_on_haiku() {
        let cfg = RoutingConfig::default();
        let mut ps = five();
        for (name, model) in [
            ("haiku", "claude-haiku-5-5"),
            ("haikuhi", "claude-haiku-5-5"),
        ] {
            let e = crate::catalog::Catalog::builtin()
                .get(model)
                .unwrap()
                .clone();
            ps.insert(
                name.into(),
                Profile {
                    provider: "anthropic".into(),
                    model: model.into(),
                    unusable: None,
                    short_cost: Some(short_cost(&e)),
                    input_per_mtok: Some(e.input_per_mtok),
                    vision: e.vision,
                    restricted: false,
                },
            );
        }
        let on = |mode: &str| {
            let v = verdict(mode, 0.9);
            brief_base(&cfg, &ps, &ask(&v, "fable", 0), false).profile
        };
        assert_eq!(on("routine_coding"), "haikuhi");
        assert_eq!(on("trivial"), "haiku");
        // A config that moved haikuhi aside still gets it first.
        let mut moved = RoutingConfig::default();
        moved.classes.routine_coding.profiles = vec!["sonnet".into()];
        let v = verdict("routine_coding", 0.9);
        assert_eq!(
            brief_base(&moved, &ps, &ask(&v, "fable", 0), false).profile,
            "haikuhi"
        );
    }

    /// A detour's class is a base: "thanks" does not leave the task on Fable.
    #[test]
    fn a_trivial_brief_is_a_base_not_a_detour() {
        let d = base("trivial", 0.9, "fable", None);
        assert_eq!(
            (d.profile.as_str(), d.switch, d.detour),
            ("glm", true, false)
        );
    }

    /// Under its bar, or a class that is the model's own, the parent's stays.
    #[test]
    fn an_unsure_brief_and_a_chat_brief_keep_the_parents_model() {
        let d = base("routine_coding", 0.1, "fable", None);
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("fable", Reason::Unsure, false)
        );
        let d = base("chat", 0.9, "fable", None);
        assert_eq!((d.profile.as_str(), d.switch), ("fable", false));
    }

    /// The place's cap caps a brief's pick too.
    #[test]
    fn the_places_cap_caps_a_briefs_pick() {
        let ps = five();
        let cap = Some(ps["sonnet"].short_cost.unwrap());
        let d = base("sophisticated", 0.9, "sonnet", cap);
        assert_eq!(d.profile, "sonnet");
        assert_eq!(d.reason, Reason::Capped);
        assert!(!d.switch);
    }

    /// Open (theseus-ej6bj): nothing is fixed to a profile, and a config that
    /// moved `haikuhi` out of the class list does not get it back.
    #[test]
    fn an_open_brief_starts_where_its_class_sends_it() {
        let ps = five();
        let mut moved = RoutingConfig::default();
        moved.classes.routine_coding.profiles = vec!["sonnet".into()];
        let v = verdict("routine_coding", 0.9);
        let d = brief_base(&moved, &ps, &ask(&v, "fable", 0), true);
        assert_eq!(d.profile, "sonnet");
    }
}
