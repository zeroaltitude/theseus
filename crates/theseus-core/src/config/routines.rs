//! `[routines]` (theseus-pvm5c): recurring work that is its own execution.
//! Every key has a default, so an owner's config names none unless it
//! differs.
//!
//! - `default_run_usd`: the most one run may spend when a routine names none
//!   (0.50); it is carved from what the routine's month has left.
//! - `default_month_usd`: a routine's month, reset on the 1st of the local
//!   month, when it names none (10).
//! - `max_run_usd`, `max_month_usd`: a ceiling on the run and month figures a routine
//!   names for itself (5 and 50). The two defaults above are not caps: a routine
//!   that names its own figure is taken at it unless a ceiling here is lower, and it
//!   is refused, never clamped, saying which key bounds it. An unconfigured daemon
//!   therefore refuses a routine a model proposes at $500 a month; an owner who wants
//!   more raises the key.
//! - `max_routines`: the most live routines the daemon holds (50).
//! - `run_deadline_secs`: how long a question a routine's run asks outside its
//!   grant may wait (900); past it the call is dropped with a notice and the
//!   run goes on without the tool (asked again, the run returns to its
//!   schedule), never blocked (theseus-v2dnc).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutinesConfig {
    #[serde(default = "default_run_usd")]
    pub default_run_usd: f64,
    #[serde(default = "default_month_usd")]
    pub default_month_usd: f64,
    #[serde(default = "default_max_run_usd")]
    pub max_run_usd: f64,
    #[serde(default = "default_max_month_usd")]
    pub max_month_usd: f64,
    #[serde(default = "default_max_routines")]
    pub max_routines: usize,
    #[serde(default = "default_run_deadline_secs")]
    pub run_deadline_secs: u64,
}

fn default_run_deadline_secs() -> u64 {
    900
}

fn default_run_usd() -> f64 {
    0.50
}
fn default_month_usd() -> f64 {
    10.0
}
fn default_max_run_usd() -> f64 {
    5.0
}
fn default_max_month_usd() -> f64 {
    50.0
}
fn default_max_routines() -> usize {
    50
}

impl Default for RoutinesConfig {
    fn default() -> Self {
        Self {
            default_run_usd: default_run_usd(),
            default_month_usd: default_month_usd(),
            max_run_usd: default_max_run_usd(),
            max_month_usd: default_max_month_usd(),
            max_routines: default_max_routines(),
            run_deadline_secs: default_run_deadline_secs(),
        }
    }
}

impl RoutinesConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if !(crate::routine::usd_ok(self.default_run_usd)
            && crate::routine::usd_ok(self.default_month_usd))
        {
            bail!("routines.default_run_usd and default_month_usd must be above 0");
        }
        if self.default_run_usd > self.default_month_usd {
            bail!(
                "routines.default_run_usd ({}) is more than default_month_usd ({}): a run could \
                 never be carved",
                self.default_run_usd,
                self.default_month_usd
            );
        }
        for (key, ceiling) in [
            ("max_run_usd", self.max_run_usd),
            ("max_month_usd", self.max_month_usd),
        ] {
            if !crate::routine::usd_ok(ceiling) {
                bail!("routines.{key} must be above 0");
            }
        }
        if self.default_run_usd > self.max_run_usd || self.default_month_usd > self.max_month_usd {
            bail!(
                "routines.max_run_usd ({}) / max_month_usd ({}) are below the defaults a routine \
                 that names none gets ({} a run, {} a month): raise the ceiling or lower the default",
                self.max_run_usd,
                self.max_month_usd,
                self.default_run_usd,
                self.default_month_usd
            );
        }
        if self.max_routines == 0 {
            bail!("routines.max_routines is 0");
        }
        if self.run_deadline_secs == 0 {
            bail!("routines.run_deadline_secs is 0: a question outside the grant could never wait");
        }
        Ok(())
    }
}
