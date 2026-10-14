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
//! - `[self.budget]`: the self budget's lines, an empty table until the
//!   self-budget row gives it `branch_usd` and `day_usd`.
//! - `repo` (theseus-pw1q.8): Theseus's own source checkout, which self
//!   branches are cut from (its `origin/main`). Unset, the default: no self
//!   branches, and `self.propose_change` says so.
//! - `remote` (theseus-pw1q.8): the remote `self/*` branches are pushed to,
//!   with the credential git already has for it (none is read here). Unset,
//!   the default: they stay in the repo. Choosing it is the owner's: a push
//!   needs a credential, and choosing one is a permission change.

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

/// `[self] digest`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfDigest {
    #[default]
    Weekly,
    Off,
}

/// `[self.budget]`: no key yet (the self-budget row adds its two).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfBudgetConfig {}

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
                && t.contains("# [self.budget]"),
            "the template names [self], its two keys and [self.budget]"
        );
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
    }

    /// `[self] repo` and `remote` are unset by default (no self branches,
    /// none pushed), the template names both, and each reads as written.
    #[test]
    fn the_self_repo_and_remote_are_unset_by_default() {
        let none = Config::from_toml("").unwrap().self_improve;
        assert_eq!((none.repo, none.remote), (None, None));
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("\n# repo = \"") && t.contains("\n# remote = \"origin\""),
            "the template names [self] repo and remote"
        );
        let cfg =
            Config::from_toml("[self]\nmode = \"act\"\nrepo = \"/srv/ship\"\nremote = \"fleet\"\n")
                .unwrap()
                .self_improve;
        assert_eq!(cfg.repo.as_deref(), Some("/srv/ship"));
        assert_eq!(cfg.remote.as_deref(), Some("fleet"));
    }
}
