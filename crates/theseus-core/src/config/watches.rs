//! `[watches]` (theseus-f2r2b): waiting on the world with no model. A watch
//! is a check (a command, a fetch, a file) run on a timer until a condition
//! holds (`watch.rs`). Every key has a default. The most watches the daemon
//! holds is `[kernel] max_watches` (20), with the other wake limits.
//!
//! - `default_every`: how often a watch checks when it does not say (5m).
//! - `min_every`: the most often one may (1m).
//! - `max_days`: the longest one waits before it gives up (30), and the
//!   default when it does not say.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchesConfig {
    #[serde(default = "default_every")]
    pub default_every: String,
    #[serde(default = "default_min_every")]
    pub min_every: String,
    #[serde(default = "default_max_days")]
    pub max_days: u64,
}

fn default_every() -> String {
    "5m".into()
}
fn default_min_every() -> String {
    "1m".into()
}
fn default_max_days() -> u64 {
    30
}

impl Default for WatchesConfig {
    fn default() -> Self {
        Self {
            default_every: default_every(),
            min_every: default_min_every(),
            max_days: default_max_days(),
        }
    }
}

impl WatchesConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The spans in milliseconds: (default, minimum).
    pub fn spans_ms(&self) -> Result<(u64, u64)> {
        let span = |key: &str, s: &str| -> Result<u64> {
            theseus_kernel::Every::parse(s)
                .map(|e| e.nominal_ms())
                .map_err(|e| anyhow::anyhow!("watches.{key}: {e}"))
        };
        Ok((
            span("default_every", &self.default_every)?,
            span("min_every", &self.min_every)?,
        ))
    }

    pub fn validate(&self) -> Result<()> {
        let (default, min) = self.spans_ms()?;
        if min < 60_000 {
            bail!("watches.min_every is under a minute: a watch checks every minute at the most often");
        }
        if default < min {
            bail!("watches.default_every is shorter than watches.min_every");
        }
        if self.max_days == 0 || self.max_days > 365 {
            bail!("watches.max_days must be from 1 to 365");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[watches]` is the default, and a note may change it.
    #[test]
    fn the_templates_watches_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.watches, WatchesConfig::default());
        assert_eq!(cfg.watches.spans_ms().unwrap(), (300_000, 60_000));
        assert_eq!(cfg.watches.max_days, 30);
        assert!(crate::Config::EXAMPLE_TOML.contains("# [watches]\n# default_every = \"5m\""));
        let own = crate::Config::from_toml("[watches]\ndefault_every = \"10m\"\n").unwrap();
        assert_eq!(own.watches.spans_ms().unwrap().0, 600_000);
        let secrets = "";
        assert!(
            crate::Config::parse(&format!("{secrets}\n[watches]\nmin_every = \"30s\"\n")).is_err()
        );
        assert!(crate::Config::from_toml("[watches]\nbogus = 1\n").is_err());
    }
}
