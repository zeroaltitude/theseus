//! `[initiative]` (theseus-slvl.9): what Theseus says first, in its persona's
//! voice. A message to the owner that the daemon starts itself and that is
//! more than a status line (a morning note, a consolidation finding) is
//! written by a short model call that carries the persona's file and the
//! standing block, off every owner turn's path. A wake's result and a task's
//! report are turns already and speak as the session's persona.
//! Every key has a default.
//!
//! - `profile`: the profile whose model writes them, a cheap one. Unset: the
//!   live profile.
//! - `day_usd`: what those calls may reserve and spend a local day (0.50). A
//!   message past it is not written by a model: it goes out as its template
//!   (`initiative/lines.rs`), which reads the persona's name and emoji.
//! - `morning_hour`: the local hour from which the day's one morning note may
//!   go (8). It goes only on a day something happened, and only when the
//!   persona's initiative lets it.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitiativeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default = "default_day_usd")]
    pub day_usd: f64,
    #[serde(default = "default_morning_hour")]
    pub morning_hour: u32,
}

fn default_day_usd() -> f64 {
    0.50
}
fn default_morning_hour() -> u32 {
    8
}

impl Default for InitiativeConfig {
    fn default() -> Self {
        Self {
            profile: None,
            day_usd: default_day_usd(),
            morning_hour: default_morning_hour(),
        }
    }
}

impl InitiativeConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if !self.day_usd.is_finite() || self.day_usd < 0.0 {
            bail!("initiative.day_usd must be 0 or more (0 writes no message with a model)");
        }
        if self.morning_hour > 23 {
            bail!("initiative.morning_hour is an hour of the day, 0 to 23");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_owners() {
        let c = InitiativeConfig::default();
        assert_eq!(
            (c.day_usd, c.morning_hour, c.profile.clone()),
            (0.50, 8, None)
        );
        assert!(c.is_default());
        assert!(c.validate().is_ok());
    }

    #[test]
    fn a_bad_value_is_refused_by_name() {
        let with = |day_usd, morning_hour| InitiativeConfig {
            day_usd,
            morning_hour,
            ..Default::default()
        };
        let why = |c: InitiativeConfig| c.validate().unwrap_err().to_string();
        assert!(why(with(-1.0, 8)).contains("day_usd"));
        assert!(with(0.0, 8).validate().is_ok(), "zero turns the model off");
        assert!(why(with(0.5, 24)).contains("morning_hour"));
    }
}
