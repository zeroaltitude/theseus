//! `[threads]` (theseus-nhew): threads on virtual places `thread:<slug>`.
//! Every key has a default, so an owner's config names none of them unless
//! it differs. The rows built on the record (`thread-context`,
//! `thread-recall`, `thread-route`) add their keys here.
//!
//! - `enabled`: threads can be made and addressed (true). Off, `thread.new`
//!   and a `turn.submit` that names a thread are refused; the records stay.
//! - `max_live`: the most active and paused threads at once (50); a thread
//!   made past it is refused until one is done.
//! - `cross_recall_penalty` (0.35) and `cross_recall_bar` (0.75), the thread
//!   distance (theseus-f2g0): in a threaded session, recall's candidate from
//!   another thread pays that share of its score and is admitted only when
//!   its score is above the bar's share of the index's best, unless the
//!   message names the thread (its slug, its title, or one of its working
//!   set's entities). Unthreaded sessions recall as before.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_live")]
    pub max_live: u32,
    #[serde(default = "default_penalty")]
    pub cross_recall_penalty: f64,
    #[serde(default = "default_bar")]
    pub cross_recall_bar: f64,
}

fn default_enabled() -> bool {
    true
}
fn default_max_live() -> u32 {
    50
}
fn default_penalty() -> f64 {
    0.35
}
fn default_bar() -> f64 {
    0.75
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            cross_recall_penalty: default_penalty(),
            cross_recall_bar: default_bar(),
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
        for (key, v) in [
            ("cross_recall_penalty", self.cross_recall_penalty),
            ("cross_recall_bar", self.cross_recall_bar),
        ] {
            if !(0.0..=1.0).contains(&v) {
                bail!("threads.{key} = {v} is outside 0 to 1");
            }
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
        assert!(crate::Config::from_toml("[threads]\nbogus = 1\n").is_err());
        assert_eq!(cfg.threads.cross_recall_penalty, 0.35);
        assert_eq!(cfg.threads.cross_recall_bar, 0.75);
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_penalty = 0.35"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_bar = 0.75"));
        for bad in ["cross_recall_penalty = 1.5", "cross_recall_bar = -0.1"] {
            let note = format!("{secrets}\n[threads]\n{bad}\n");
            assert!(crate::Config::parse(&note).is_err(), "{bad}");
        }
    }
}
