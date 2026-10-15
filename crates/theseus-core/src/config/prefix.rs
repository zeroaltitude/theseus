//! `[prefix]` (theseus-rr15): the prefix library, `crate::prefix`. Every key
//! has a default, so an owner's config names none of them unless it differs.
//!
//! - `enabled`: the library picks layer 3 at compile points (true). Off, each
//!   session renders its own layers, as before the library (the threadbench's
//!   arm A1n), and nothing is read or written for it.
//! - `max_bytes`: the library's resident size (10 MiB): past it the header
//!   read least recently is retired first.
//! - `act` and `confirm`: the fit bands (0.90, 0.60). A header whose fit
//!   reaches `confirm` is reused; one below `act` is watched as a possible
//!   false fit; below `confirm` the library mints when minting pays, else it
//!   reuses the nearest coarser header.
//! - `candidates`: the nearest headers by sketch a pick scores (16).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
    #[serde(default = "default_act")]
    pub act: f64,
    #[serde(default = "default_confirm")]
    pub confirm: f64,
    #[serde(default = "default_candidates")]
    pub candidates: u32,
}

fn default_enabled() -> bool {
    true
}
fn default_max_bytes() -> u64 {
    10 * 1024 * 1024
}
fn default_act() -> f64 {
    0.90
}
fn default_confirm() -> f64 {
    0.60
}
fn default_candidates() -> u32 {
    16
}

impl Default for PrefixConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_bytes: default_max_bytes(),
            act: default_act(),
            confirm: default_confirm(),
            candidates: default_candidates(),
        }
    }
}

impl PrefixConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.confirm) || !(0.0..=1.0).contains(&self.act) {
            bail!("prefix.act and prefix.confirm are fits, between 0 and 1");
        }
        if self.confirm > self.act {
            bail!("prefix.confirm is above prefix.act: the bands would cross");
        }
        if self.max_bytes < 64 * 1024 {
            bail!(
                "prefix.max_bytes is under 64 KiB: the library could not hold one project's header"
            );
        }
        if self.candidates == 0 {
            bail!("prefix.candidates is 0: use enabled = false to turn the library off");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[prefix]` is the default, and a note may change it.
    #[test]
    fn the_templates_prefix_section_is_the_default() {
        let template = include_str!("../../config/theseus.example.toml");
        let at = template
            .find("\n# [prefix]\n")
            .expect("the template has [prefix]");
        let section: String = template[at + 1..]
            .lines()
            .skip(1)
            .take_while(|l| l.starts_with("# ") && !l.starts_with("# ---"))
            .filter_map(|l| l.strip_prefix("# "))
            .filter(|l| l.contains(" = "))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: PrefixConfig = toml::from_str(&section).unwrap();
        assert_eq!(parsed, PrefixConfig::default());
        assert!(parsed.validate().is_ok());
        let off: PrefixConfig = toml::from_str("enabled = false").unwrap();
        assert!(!off.enabled && off.max_bytes == 10 * 1024 * 1024);
        let crossed: PrefixConfig = toml::from_str("act = 0.5").unwrap();
        assert!(crossed.validate().is_err());
    }
}
