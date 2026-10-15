//! `[route]` (own-benchmark, theseus-pw1q.12): your own benchmark's sampler.
//! Nightly, a few of the owner's recent turns per (model, effort) cell are
//! replayed one rung down the routing table and judged by `compare.v1`
//! (`theseus route report` reads them). It spends only under its own day cap
//! and writes no routing: a yardstick, not a self-change.
//!
//! - `replay_day_usd`: the most the sampler's replays and their judgments
//!   may cost in a local day (1.00). 0 turns the sampler off.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteConfig {
    #[serde(default = "default_replay_day_usd")]
    pub replay_day_usd: f64,
}

fn default_replay_day_usd() -> f64 {
    1.0
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            replay_day_usd: default_replay_day_usd(),
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
    }
}
