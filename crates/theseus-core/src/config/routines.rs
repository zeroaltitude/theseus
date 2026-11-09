//! `[routines]` (theseus-pvm5c): recurring work that is its own execution.
//! Every key has a default, so an owner's config names none unless it
//! differs.
//!
//! - `default_run_usd`: the most one run may spend when a routine names none
//!   (0.50); it is carved from what the routine's month has left.
//! - `default_month_usd`: a routine's month, reset on the 1st of the local
//!   month, when it names none (10).
//! - `max_run_usd`, `max_month_usd`: a ceiling on the run and month figures a routine
//!   names for itself (unset: none). The two defaults above are not caps: a routine
//!   that names its own figure is taken at it unless a ceiling here is lower, and it
//!   is refused, never clamped, saying which key bounds it.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_run_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_month_usd: Option<f64>,
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
fn default_max_routines() -> usize {
    50
}

impl Default for RoutinesConfig {
    fn default() -> Self {
        Self {
            default_run_usd: default_run_usd(),
            default_month_usd: default_month_usd(),
            max_run_usd: None,
            max_month_usd: None,
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
            if ceiling.is_some_and(|c| !crate::routine::usd_ok(c)) {
                bail!("routines.{key} must be above 0 when it is set");
            }
        }
        if self.max_run_usd.is_some_and(|c| self.default_run_usd > c)
            || self
                .max_month_usd
                .is_some_and(|c| self.default_month_usd > c)
        {
            bail!("routines.max_run_usd / max_month_usd are below the defaults a routine that names none gets");
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
