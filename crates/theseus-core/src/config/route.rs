//! `[route]` (own-benchmark, theseus-pw1q.12): your own benchmark's sampler.
//! Nightly, a few of the owner's recent turns per (model, effort) cell are
//! replayed one rung down the routing table and judged by `compare.v1`
//! (`theseus route report` reads them). It spends only under its own day cap
//! and writes no routing: a yardstick, not a self-change.
//!
//! - `replay_day_usd`: the most the sampler's replays and their judgments
//!   may cost in a local day (1.00). 0 turns the sampler off.
//! - `whole_*` (theseus-pw1q.20): a whole-turn replay's caps, each a ceiling
//!   on one replay: `whole_loops` model calls (24), `whole_seconds` of wall
//!   clock (300), `whole_usd` of model spend (0.50, also held to what is
//!   left of `replay_day_usd` in the sampler), `whole_tree_mb` for the
//!   scratch copy of the directory (256). `whole_label_bar` (0.9) is the
//!   confidence a judged whole-turn match needs to become a machine label
//!   for routing's learning loop; 0 writes none.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteConfig {
    #[serde(default = "default_replay_day_usd")]
    pub replay_day_usd: f64,
    #[serde(default = "default_whole_loops")]
    pub whole_loops: u32,
    #[serde(default = "default_whole_seconds")]
    pub whole_seconds: u64,
    #[serde(default = "default_whole_usd")]
    pub whole_usd: f64,
    #[serde(default = "default_whole_tree_mb")]
    pub whole_tree_mb: u64,
    #[serde(default = "default_whole_label_bar")]
    pub whole_label_bar: f64,
}

fn default_replay_day_usd() -> f64 {
    1.0
}

fn default_whole_loops() -> u32 {
    24
}

fn default_whole_seconds() -> u64 {
    300
}

fn default_whole_usd() -> f64 {
    0.5
}

fn default_whole_tree_mb() -> u64 {
    256
}

fn default_whole_label_bar() -> f64 {
    0.9
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            replay_day_usd: default_replay_day_usd(),
            whole_loops: default_whole_loops(),
            whole_seconds: default_whole_seconds(),
            whole_usd: default_whole_usd(),
            whole_tree_mb: default_whole_tree_mb(),
            whole_label_bar: default_whole_label_bar(),
        }
    }
}

impl RouteConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if !(0.0..=100.0).contains(&self.replay_day_usd) {
            bail!(
                "route.replay_day_usd = {} is outside 0 to 100",
                self.replay_day_usd
            );
        }
        if !(1..=200).contains(&self.whole_loops) {
            bail!(
                "route.whole_loops = {} is outside 1 to 200",
                self.whole_loops
            );
        }
        if !(1..=3600).contains(&self.whole_seconds) {
            bail!(
                "route.whole_seconds = {} is outside 1 to 3600",
                self.whole_seconds
            );
        }
        if !(0.0..=100.0).contains(&self.whole_usd) {
            bail!("route.whole_usd = {} is outside 0 to 100", self.whole_usd);
        }
        if !(1..=4096).contains(&self.whole_tree_mb) {
            bail!(
                "route.whole_tree_mb = {} is outside 1 to 4096",
                self.whole_tree_mb
            );
        }
        if !(0.0..=1.0).contains(&self.whole_label_bar) {
            bail!(
                "route.whole_label_bar = {} is outside 0 to 1",
                self.whole_label_bar
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[route]` is the default, and a note may change it.
    #[test]
    fn the_templates_route_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.route, RouteConfig::default());
        assert!((cfg.route.replay_day_usd - 1.0).abs() < f64::EPSILON);
        assert!(crate::Config::EXAMPLE_TOML.contains("# [route]\n# replay_day_usd = 1.0"));
        let off = crate::Config::from_toml("[route]\nreplay_day_usd = 0\n").unwrap();
        assert!(off.route.replay_day_usd == 0.0);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        assert!(
            crate::Config::parse(&format!("{secrets}\n[route]\nreplay_day_usd = -1\n")).is_err()
        );
        assert!(crate::Config::from_toml("[route]\nbogus = 1\n").is_err());
        // The whole-turn replay's caps (theseus-pw1q.20): defaults in the
        // template, each bounded.
        assert_eq!(cfg.route.whole_loops, 24);
        assert_eq!(cfg.route.whole_seconds, 300);
        assert!((cfg.route.whole_usd - 0.5).abs() < f64::EPSILON);
        assert_eq!(cfg.route.whole_tree_mb, 256);
        assert!((cfg.route.whole_label_bar - 0.9).abs() < f64::EPSILON);
        for bad in [
            "whole_loops = 0",
            "whole_seconds = 0",
            "whole_usd = -1",
            "whole_tree_mb = 0",
            "whole_label_bar = 1.5",
        ] {
            assert!(
                crate::Config::parse(&format!("{secrets}\n[route]\n{bad}\n")).is_err(),
                "{bad}"
            );
        }
    }
}
