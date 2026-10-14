//! `[self]` (theseus-pw1q.4, theseus-pw1q.2): self-improvement, off by
//! default. Read at the start, like the AWS lines: a change takes a restart.
//!
//! - `mode`: `"off"` (the default: nothing self-directed runs, whatever the
//!   kill switch says) or `"act"` (self steps run while the owner has
//!   released the kill switch; a store that has never seen the owner's
//!   resume is halted, so `"act"` alone starts nothing).
//! - `digest`: `"weekly"` (the default: with `mode = "act"`, the week's
//!   "What Theseus changed about itself" goes to the owner's DM once a week)
//!   or `"off"`.
//! - `[self.budget]` (theseus-pw1q.3): the one spend account for all
//!   self-improvement. `branch_usd` ($10) is a self-branch's line, `day_usd`
//!   ($30) the local day's, across every self loop together. Part of the
//!   hard keel: raising either is the owner's.

use serde::{Deserialize, Deserializer, Serialize};
pub use theseus_protocol::rsi::SelfMode;

/// `[self]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfConfig {
    #[serde(default, deserialize_with = "mode")]
    pub mode: SelfMode,
    #[serde(default, deserialize_with = "digest")]
    pub digest: SelfDigest,
    #[serde(default)]
    pub budget: SelfBudgetConfig,
}

/// `[self] digest`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfDigest {
    #[default]
    Weekly,
    Off,
}

/// `[self.budget]`: the two spend lines (theseus-pw1q.3), read at the start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfBudgetConfig {
    /// What one self branch may spend, all its loops together.
    #[serde(default = "default_branch_usd")]
    pub branch_usd: f64,
    /// What all self-improvement may spend in one local day.
    #[serde(default = "default_day_usd")]
    pub day_usd: f64,
}

fn default_branch_usd() -> f64 {
    10.0
}

fn default_day_usd() -> f64 {
    30.0
}

impl Default for SelfBudgetConfig {
    fn default() -> Self {
        Self {
            branch_usd: default_branch_usd(),
            day_usd: default_day_usd(),
        }
    }
}

impl SelfConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether the weekly digest is posted: only while self steps may run.
    pub fn posts_digest(&self) -> bool {
        self.mode == SelfMode::Act && self.digest == SelfDigest::Weekly
    }
}

/// One of `allowed`, or an error naming the key and its values.
fn one_of<'de, D: Deserializer<'de>, T: Copy>(
    d: D,
    key: &str,
    allowed: &[(&str, T)],
) -> Result<T, D::Error> {
    let s = String::deserialize(d)?;
    allowed
        .iter()
        .find(|(name, _)| *name == s)
        .map(|(_, v)| *v)
        .ok_or_else(|| {
            let names: Vec<String> = allowed.iter().map(|(n, _)| format!("\"{n}\"")).collect();
            serde::de::Error::custom(format!(
                "[self] {key} must be {}, not \"{s}\"",
                names.join(" or ")
            ))
        })
}

fn mode<'de, D: Deserializer<'de>>(d: D) -> Result<SelfMode, D::Error> {
    one_of(d, "mode", &[("off", SelfMode::Off), ("act", SelfMode::Act)])
}

fn digest<'de, D: Deserializer<'de>>(d: D) -> Result<SelfDigest, D::Error> {
    one_of(
        d,
        "digest",
        &[("weekly", SelfDigest::Weekly), ("off", SelfDigest::Off)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    /// `[self]` is off by default, the template carries it as the default,
    /// and an unknown mode is refused at load with the key's name.
    #[test]
    fn the_self_section_is_off_by_default_and_refuses_an_unknown_mode() {
        let none = Config::from_toml("").unwrap();
        assert_eq!(none.self_improve, SelfConfig::default());
        assert_eq!(none.self_improve.mode, SelfMode::Off);
        assert_eq!(none.self_improve.digest, SelfDigest::Weekly);
        assert!(!none.self_improve.posts_digest());
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("# [self]\n# mode = \"off\"")
                && t.contains("# digest = \"weekly\"")
                && t.contains("# [self.budget]")
                && t.contains("# branch_usd = 10.0")
                && t.contains("# day_usd = 30.0"),
            "the template names [self], its two keys and [self.budget]"
        );
        assert_eq!(none.self_improve.budget.branch_usd, 10.0);
        assert_eq!(none.self_improve.budget.day_usd, 30.0);
        let cfg =
            Config::from_toml("[self]\nmode = \"act\"\ndigest = \"off\"\n[self.budget]\n").unwrap();
        assert_eq!(cfg.self_improve.mode, SelfMode::Act);
        assert_eq!(cfg.self_improve.digest, SelfDigest::Off);
        assert!(!cfg.self_improve.posts_digest());
        let act = Config::from_toml("[self]\nmode = \"act\"\n").unwrap();
        assert!(act.self_improve.posts_digest());
        let e = format!(
            "{:#}",
            Config::from_toml("[self]\nmode = \"on\"\n").unwrap_err()
        );
        assert!(
            e.contains("[self] mode must be \"off\" or \"act\", not \"on\""),
            "{e}"
        );
        let e = format!(
            "{:#}",
            Config::from_toml("[self]\ndigest = \"daily\"\n").unwrap_err()
        );
        assert!(e.contains("[self] digest must be"), "{e}");
        assert!(Config::from_toml("[self]\nbudget_usd = 1.0\n").is_err());
        let b = Config::from_toml("[self.budget]\nbranch_usd = 4.5\nday_usd = 12.0\n").unwrap();
        assert_eq!(b.self_improve.budget.branch_usd, 4.5);
        assert_eq!(b.self_improve.budget.day_usd, 12.0);
        assert!(Config::from_toml("[self.budget]\nweek_usd = 1.0\n").is_err());
    }
}
