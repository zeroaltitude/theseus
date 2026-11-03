//! A task's base from its brief (theseus-ypnxt.7). A task starts with no
//! message of its own, so the verdict the route pack gives its brief picks the model
//! it runs on: Jev's pick, by [`decide`]'s rule with nothing to keep warm and
//! nothing held.
//!
//! The pick is the task's base, not a move from its parent's model: it is
//! kept as the session's target and routing's `from` is left empty. A `short`
//! pick is a base here too: a task has no "next message" to leave the model for.

use super::{decide, Ask, Decision, HoldNext, Profiles, Reason};
use crate::config::RoutingConfig;

/// The rule's answer for the brief `a.verdict` judged, `a.base` being the
/// model the task would otherwise run on (its parent's). `switch` says the
/// task's base is another profile.
pub fn brief_base(cfg: &RoutingConfig, profiles: &Profiles, a: &Ask<'_>) -> Decision {
    let fresh = Ask {
        est_tokens: 0,
        hold: None,
        ..a.clone()
    };
    let mut d = decide(cfg, profiles, &fresh);
    if d.detour {
        d = Decision {
            detour: false,
            switch: true,
            reason: Reason::Verdict,
            hold: HoldNext::Clear,
            ..d
        };
    }
    d
}
