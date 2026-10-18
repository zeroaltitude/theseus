//! A task's base from its brief (theseus-ypnxt.7). A task starts with no
//! message of its own, so the verdict route.v6 gives its brief picks the model
//! it runs on: the class's first usable profile under the place's cap, by
//! [`decide`]'s rule with nothing to keep warm and nothing held.
//!
//! The pick is the task's base, not a move from its parent's model: it is
//! kept as the session's target and routing's `from` is left empty, so only
//! Jev elevates the task from it and a step back down never goes below it.
//! A detour's class (`trivial`, `quick`) is a base here too: a task has no
//! "next message" to leave the model for.

use super::{decide, Ask, Decision, HoldNext, Profiles, Reason};
use crate::config::RoutingConfig;

/// The rule's answer for the brief `a.verdict` judged, `a.base` being the
/// model the task would otherwise run on (its parent's). `switch` says the
/// task's base is another profile.
pub fn brief_base(cfg: &RoutingConfig, profiles: &Profiles, a: &Ask<'_>) -> Decision {
    let fresh = Ask {
        est_tokens: 0,
        hold: None,
        from: None,
        ..a.clone()
    };
    let mut d = decide(cfg, profiles, &fresh);
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
}
