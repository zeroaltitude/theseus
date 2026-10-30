//! `[card]` (theseus-slvl.6): the owner card filled from the owner's imported
//! history. A job reads the archive's feedback and SOP notes, USER-style
//! files and `decision_or_preference` episodes, and a cheap model states each
//! cluster as one short fact or preference with the episodes it rests on; the
//! owner keeps, edits or drops each one. Nothing is added by itself. Every key
//! has a default, so an owner's config names none unless it differs.
//!
//! - `profile`: the profile whose model states the clusters (`haiku`).
//! - `day_usd`: the job's spend a local day; a call whose worst case would pass
//!   what is left is not made (2.0). 0 turns the job off.
//! - `max_open`: the most proposals waiting at once (30).
//! - `ask_days`: a proposal nobody answers in this long becomes "Not this" (14).
//! - `every_days`: the nightly run (at `[judge] learning_hour`) happens at most
//!   once in this many days, until the owner says done (7). 0 turns the nightly
//!   off; `theseus card propose` still runs it.
//! - `observe`: live learning (theseus-6f8t0): the exchange-end observer, which
//!   keeps what the owner says about the owner's own setup, preferences and
//!   decisions, and the nightly live pass that turns those into questions (on).
//!   Off also when `day_usd` is 0.
//! - `live_per_night`: the most live questions one nightly pass proposes (5).
//! - `stale_per_night`: of those, the most Stale questions, an observation that
//!   contradicts a live card record (2; at most `live_per_night`).
//! - `pattern_sessions`: the distinct conversations a Pattern question needs
//!   to have seen the same preference or decision in (3).
//! - `dm_per_day`: the most live questions posted to the owner's DM a day (3;
//!   0: they are asked at the CLI only).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardConfig {
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default = "default_day_usd")]
    pub day_usd: f64,
    #[serde(default = "default_max_open")]
    pub max_open: u32,
    #[serde(default = "default_ask_days")]
    pub ask_days: u32,
    #[serde(default = "default_every_days")]
    pub every_days: u32,
    #[serde(default = "yes")]
    pub observe: bool,
    #[serde(default = "default_live_per_night")]
    pub live_per_night: u32,
    #[serde(default = "default_stale_per_night")]
    pub stale_per_night: u32,
    #[serde(default = "default_pattern_sessions")]
    pub pattern_sessions: u32,
    #[serde(default = "default_dm_per_day")]
    pub dm_per_day: u32,
}

fn default_day_usd() -> f64 {
    2.0
}
fn default_max_open() -> u32 {
    30
}
fn default_ask_days() -> u32 {
    14
}
fn default_every_days() -> u32 {
    7
}
fn yes() -> bool {
    true
}
fn default_live_per_night() -> u32 {
    5
}
fn default_stale_per_night() -> u32 {
    2
}
fn default_pattern_sessions() -> u32 {
    3
}
fn default_dm_per_day() -> u32 {
    3
}

impl Default for CardConfig {
    fn default() -> Self {
        Self {
            profile: None,
            day_usd: default_day_usd(),
            max_open: default_max_open(),
            ask_days: default_ask_days(),
            every_days: default_every_days(),
            observe: yes(),
            live_per_night: default_live_per_night(),
            stale_per_night: default_stale_per_night(),
            pattern_sessions: default_pattern_sessions(),
            dm_per_day: default_dm_per_day(),
        }
    }
}

impl CardConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(self.day_usd.is_finite() && self.day_usd >= 0.0) {
            bail!(
                "[card] day_usd is dollars a day, 0 or more (0 turns the job off), not {}",
                self.day_usd
            );
        }
        if self.max_open == 0 || self.max_open > 200 {
            bail!("[card] max_open is 1 to 200, not {}", self.max_open);
        }
        if self.ask_days == 0 || self.ask_days > 365 {
            bail!("[card] ask_days is 1 to 365, not {}", self.ask_days);
        }
        if self.every_days > 365 {
            bail!("[card] every_days is 0 to 365, not {}", self.every_days);
        }
        if self.live_per_night > 50 {
            bail!(
                "[card] live_per_night is 0 to 50, not {}",
                self.live_per_night
            );
        }
        if self.stale_per_night > self.live_per_night {
            bail!(
                "[card] stale_per_night is 0 to live_per_night ({}), not {}",
                self.live_per_night,
                self.stale_per_night
            );
        }
        if !(2..=20).contains(&self.pattern_sessions) {
            bail!(
                "[card] pattern_sessions is 2 to 20, not {}",
                self.pattern_sessions
            );
        }
        if self.dm_per_day > 10 {
            bail!("[card] dm_per_day is 0 to 10, not {}", self.dm_per_day);
        }
        if self.profile.as_deref().is_some_and(|p| p.trim().is_empty()) {
            bail!("[card] profile names no profile");
        }
        Ok(())
    }
}
