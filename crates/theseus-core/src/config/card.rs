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

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardConfig {
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_day_usd")]
    pub day_usd: f64,
    #[serde(default = "default_max_open")]
    pub max_open: u32,
    #[serde(default = "default_ask_days")]
    pub ask_days: u32,
    #[serde(default = "default_every_days")]
    pub every_days: u32,
}

fn default_profile() -> String {
    "haiku".into()
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

impl Default for CardConfig {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            day_usd: default_day_usd(),
            max_open: default_max_open(),
            ask_days: default_ask_days(),
            every_days: default_every_days(),
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
        if self.profile.trim().is_empty() {
            bail!("[card] profile names no profile");
        }
        Ok(())
    }
}
