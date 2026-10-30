//! route.v10's `model` path (theseus-h66v4, row 3): what the daemon does with
//! Jev's three answers. Stubbed in the red commit.

use super::{Ask, Decision, HoldNext, Profiles, Reason};
use crate::config::RoutingConfig;

/// route.v10's `context` option for a message answered from the last exchanges.
pub const SHORT: &str = "short";
/// ... and for one answered from the session's compilation.
pub const FULL: &str = "full";

/// The `model` question's options, and the catalog model each names.
pub const OPTIONS: [(&str, &str); 0] = [];

/// The catalog model an option names.
pub fn model_of(_option: &str) -> Option<&'static str> {
    None
}

/// The configured profile that runs an option's model.
pub fn profile_for<'p>(_option: &str, _profiles: &'p Profiles) -> Option<&'p str> {
    None
}

/// A block's explanation for `theseus why`.
pub fn block_why(_means: &str) -> String {
    String::new()
}

/// route.v10's switch rule.
pub fn decide(_cfg: &RoutingConfig, _profiles: &Profiles, a: &Ask<'_>) -> Decision {
    Decision {
        profile: a.base.to_string(),
        reason: Reason::NoVerdict,
        detour: false,
        switch: false,
        hold: HoldNext::Keep,
    }
}
