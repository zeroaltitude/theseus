//! `[tools.defer]` (theseus-7m7w.2): which tool families a request loads.
//!
//! Every model call reads the whole tool set, and most places use a few
//! families of it. A family a place does not need is declared deferred
//! (`defer_loading` on Anthropic's API, where the model finds it by search) or
//! left out with a `tools_find` to ask for it (anywhere else). The loaded set
//! per place kind starts from `tools_defer::defaults` and is learned from the
//! ledger's use: a family used in at least `load_share` of a place kind's turns
//! over the last `window_days` loads, and one unused for `hide_after_days`
//! hides. It is recomputed only where the conversation recompiles anyway.

use serde::{Deserialize, Serialize};

/// `[tools.defer]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeferConfig {
    /// Off: every offered tool loads, as before.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Families that always load, in every place and whatever the ledger says.
    #[serde(default = "always_loaded")]
    pub always_loaded: Vec<String>,
    /// A family used in at least this share of a place kind's turns over the
    /// window loads there.
    #[serde(default = "load_share")]
    pub load_share: f64,
    /// The days of ledger the share is counted over.
    #[serde(default = "window_days")]
    pub window_days: u32,
    /// A family loaded by default and unused this many days hides.
    #[serde(default = "hide_after_days")]
    pub hide_after_days: u32,
}

fn enabled() -> bool {
    true
}

fn always_loaded() -> Vec<String> {
    vec!["files".into(), "commands".into()]
}

fn load_share() -> f64 {
    0.10
}

fn window_days() -> u32 {
    14
}

fn hide_after_days() -> u32 {
    14
}

impl Default for DeferConfig {
    fn default() -> Self {
        Self {
            enabled: enabled(),
            always_loaded: always_loaded(),
            load_share: load_share(),
            window_days: window_days(),
            hide_after_days: hide_after_days(),
        }
    }
}

impl DeferConfig {
    /// A message for the first thing wrong with the section, if any.
    pub fn check(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.load_share) {
            return Err(format!(
                "[tools.defer] load_share = {} must be between 0 and 1",
                self.load_share
            ));
        }
        if self.window_days == 0 || self.hide_after_days == 0 {
            return Err("[tools.defer] window_days and hide_after_days must be at least 1".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[tools.defer]` is the default, and a note may change a
    /// key without naming the others.
    #[test]
    fn the_templates_defer_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.tools.defer, DeferConfig::default());
        assert!(cfg.tools.defer.enabled);
        assert_eq!(cfg.tools.defer.always_loaded, ["files", "commands"]);
    }

    #[test]
    fn a_share_outside_zero_to_one_is_refused() {
        let bad = DeferConfig {
            load_share: 1.5,
            ..Default::default()
        };
        assert!(bad.check().is_err());
        let bad = DeferConfig {
            window_days: 0,
            ..Default::default()
        };
        assert!(bad.check().is_err());
        assert!(DeferConfig::default().check().is_ok());
    }
}
