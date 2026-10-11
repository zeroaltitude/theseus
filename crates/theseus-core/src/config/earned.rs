//! `[policy.earned]` (theseus-uttu): the weekly loosening card. What the
//! owner approved every time, with no decline, is offered back once a week
//! as a grant: the command runs with a notice instead of asking.

use serde::{Deserialize, Serialize};

/// `[policy.earned]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EarnedConfig {
    /// Propose grants from the record. `false`: no card is posted, and no
    /// grant is made (the ones already made keep working until revoked).
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// At most one card in this many days.
    #[serde(default = "every_days")]
    pub every_days: u64,
    /// A group is proposed only when approved at least this many times in
    /// the window, with no decline and no "should have asked".
    #[serde(default = "min_approvals")]
    pub min_approvals: u64,
    /// The window the approvals are counted in, in days.
    #[serde(default = "window_days")]
    pub window_days: u64,
}

pub const ENABLED: bool = true;
pub const EVERY_DAYS: u64 = 7;
pub const MIN_APPROVALS: u64 = 20;
pub const WINDOW_DAYS: u64 = 7;

fn enabled() -> bool {
    ENABLED
}
fn every_days() -> u64 {
    EVERY_DAYS
}
fn min_approvals() -> u64 {
    MIN_APPROVALS
}
fn window_days() -> u64 {
    WINDOW_DAYS
}

impl Default for EarnedConfig {
    fn default() -> Self {
        Self {
            enabled: ENABLED,
            every_days: EVERY_DAYS,
            min_approvals: MIN_APPROVALS,
            window_days: WINDOW_DAYS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[policy.earned]` is the default, and a note may change it.
    #[test]
    fn the_templates_earned_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.policy.earned, EarnedConfig::default());
        assert!(crate::Config::EXAMPLE_TOML.contains("[policy.earned]\nenabled = true"));
        let with = |line: &str| {
            crate::Config::from_toml(
                &crate::Config::EXAMPLE_TOML.replace("min_approvals = 20", line),
            )
        };
        assert_eq!(
            with("min_approvals = 3")
                .unwrap()
                .policy
                .earned
                .min_approvals,
            3
        );
        assert!(crate::Config::from_toml(
            &crate::Config::EXAMPLE_TOML.replace("every_days = 7", "every = 7")
        )
        .is_err());
    }
}
