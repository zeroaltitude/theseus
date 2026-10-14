//! `[threads]` (theseus-nhew): threads on virtual places `thread:<slug>`.
//! Every key has a default, so an owner's config names none of them unless
//! it differs. The rows built on the record (`thread-context`,
//! `thread-recall`, `thread-route`) add their keys here.
//!
//! - `enabled`: threads can be made and addressed (true). Off, `thread.new`
//!   and a `turn.submit` that names a thread are refused; the records stay.
//! - `max_live`: the most active and paused threads at once (50); a thread
//!   made past it is refused until one is done.
//! - `import_day_usd`: what the cards of `theseus thread import --apply` may
//!   spend a local day (2.0; theseus-71ib). One model call a thread, cited to
//!   the episodes it read; a thread past the day's line is made without a card.
//! - `import_profile`: the profile whose model writes those cards (`haiku`).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_live")]
    pub max_live: u32,
    #[serde(default = "default_import_day_usd")]
    pub import_day_usd: f64,
    #[serde(default = "default_import_profile")]
    pub import_profile: String,
}

fn default_enabled() -> bool {
    true
}
fn default_max_live() -> u32 {
    50
}
fn default_import_day_usd() -> f64 {
    2.0
}
fn default_import_profile() -> String {
    "haiku".into()
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            import_day_usd: default_import_day_usd(),
            import_profile: default_import_profile(),
        }
    }
}

impl ThreadsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if self.max_live == 0 {
            bail!("threads.max_live is 0: use enabled = false to turn threads off");
        }
        if !self.import_day_usd.is_finite() || self.import_day_usd < 0.0 {
            bail!("threads.import_day_usd is not a dollar amount of 0 or more");
        }
        if self.import_profile.trim().is_empty() {
            bail!("threads.import_profile names no profile");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[threads]` is the default, and a note may change it.
    #[test]
    fn the_templates_threads_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.threads, ThreadsConfig::default());
        assert!(cfg.threads.enabled && cfg.threads.max_live == 50);
        assert!(crate::Config::EXAMPLE_TOML.contains("# [threads]\n# enabled = true"));
        let off = crate::Config::from_toml("[threads]\nenabled = false\nmax_live = 3\n").unwrap();
        assert!(!off.threads.enabled && off.threads.max_live == 3);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        assert!(crate::Config::parse(&format!("{secrets}\n[threads]\nmax_live = 0\n")).is_err());
        assert_eq!(cfg.threads.import_day_usd, 2.0);
        assert_eq!(cfg.threads.import_profile, "haiku");
        assert!(crate::Config::EXAMPLE_TOML.contains("# import_day_usd = 2.0"));
        let pricey = crate::Config::from_toml("[threads]\nimport_day_usd = 0.5\n").unwrap();
        assert_eq!(pricey.threads.import_day_usd, 0.5);
        for bad in ["import_day_usd = -1.0", "import_profile = \"\""] {
            let toml = format!("{secrets}\n[threads]\n{bad}\n");
            assert!(crate::Config::parse(&toml).is_err(), "{bad}");
        }
        assert!(crate::Config::from_toml("[threads]\nbogus = 1\n").is_err());
    }
}
