//! `[threads]` (theseus-nhew): threads on virtual places `thread:<slug>`.
//! Every key has a default, so an owner's config names none of them unless
//! it differs. The rows built on the record (`thread-context`,
//! `thread-recall`, `thread-route`) add their keys here.
//!
//! - `enabled`: threads can be made and addressed (true). Off, `thread.new`
//!   and a `turn.submit` that names a thread are refused; the records stay.
//! - `max_live`: the most active and paused threads at once (50); a thread
//!   made past it is refused until one is done.
//! - `discord_channel`: a channel (a text channel or a forum) in the owner's
//!   guild where each active thread gets a Discord thread of its own, made at
//!   the thread's first message (theseus-sjzm); unset, none.
//! - `import_day_usd`: what the cards of `theseus thread import --apply` may
//!   spend a local day (2.0; theseus-71ib). One model call a thread, cited to
//!   the episodes it read; a thread past the day's line is made without a card.
//! - `import_profile`: the profile whose model writes those cards (`haiku`).
//! - `cross_recall_penalty` (0.35) and `cross_recall_bar` (0.75), the thread
//!   distance (theseus-f2g0): in a threaded session, recall's candidate from
//!   another thread pays that share of its score and is admitted only when
//!   its score is above the bar's share of the index's best, unless the
//!   message names the thread (its slug, its title, or one of its working
//!   set's entities). Unthreaded sessions recall as before.
//! - `sticky_half_life_secs`: how fast a place's last thread stops being the
//!   likely one for its next message (600): the router's stickiness halves
//!   every this many seconds of gap (theseus-rha2).
//! - `route_wait_ms`: the most an unaddressed message waits for Jev's choice of
//!   thread, when the router's own score does not decide (400); on timeout the
//!   router's pick stands, tagged as a guess.
//! - `ask_wait_secs`: how long a message waits for the owner's answer to
//!   "which thread?" before it runs in the place's own conversation (900).
//! - `resume_keep_turns`: the newest turns a thread keeps whole when its next
//!   message finds its cache gone (6; `thread-context`).
//! - `working_tokens`: what a thread's request stays within after such a cold
//!   resume (60,000), its newest turn aside.
//! - `persona_engage` (`shadow`): whether Jev's `persona` verdict (route.v5,
//!   theseus-c1uz) engages a persona. An explicit address (a name at the
//!   message's start, `@name`, `/as name`) always does; `shadow` records
//!   Jev's pick and keeps the prior, `act` engages it at 0.90 and shows
//!   a why from 0.60. The maintainer sets `act` after the persona holdout.
//! - `seal_recommend` (false): whether a `yes` of route.v5's `seal` question
//!   at the confirm bar or above, in a thread, proposes sealing it (a card to
//!   the owner's DM; Jev recommends, the owner seals). Off until its holdout.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Whether Jev's persona verdict engages a persona.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersonaEngage {
    /// Recorded only: the prior stands unless the message addresses one.
    #[default]
    Shadow,
    /// Engaged at the act bar, shown with its why from the confirm bar.
    Act,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_live")]
    pub max_live: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord_channel: Option<String>,
    #[serde(default = "default_import_day_usd")]
    pub import_day_usd: f64,
    #[serde(default = "default_import_profile")]
    pub import_profile: String,
    #[serde(default = "default_penalty")]
    pub cross_recall_penalty: f64,
    #[serde(default = "default_bar")]
    pub cross_recall_bar: f64,
    #[serde(default = "default_half_life")]
    pub sticky_half_life_secs: u64,
    #[serde(default = "default_route_wait")]
    pub route_wait_ms: u64,
    #[serde(default = "default_ask_wait")]
    pub ask_wait_secs: u64,
    #[serde(default = "default_resume_keep_turns")]
    pub resume_keep_turns: u32,
    #[serde(default = "default_working_tokens")]
    pub working_tokens: u64,
    #[serde(default = "default_keep_warm_hours")]
    pub keep_warm_hours: f64,
    #[serde(default)]
    pub persona_engage: PersonaEngage,
    #[serde(default)]
    pub seal_recommend: bool,
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
fn default_penalty() -> f64 {
    0.35
}
fn default_bar() -> f64 {
    0.75
}
fn default_half_life() -> u64 {
    600
}
fn default_route_wait() -> u64 {
    400
}
fn default_ask_wait() -> u64 {
    900
}
fn default_resume_keep_turns() -> u32 {
    6
}
fn default_working_tokens() -> u64 {
    60_000
}

fn default_keep_warm_hours() -> f64 {
    4.0
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            discord_channel: None,
            import_day_usd: default_import_day_usd(),
            import_profile: default_import_profile(),
            cross_recall_penalty: default_penalty(),
            cross_recall_bar: default_bar(),
            sticky_half_life_secs: default_half_life(),
            route_wait_ms: default_route_wait(),
            ask_wait_secs: default_ask_wait(),
            resume_keep_turns: default_resume_keep_turns(),
            working_tokens: default_working_tokens(),
            keep_warm_hours: default_keep_warm_hours(),
            persona_engage: PersonaEngage::default(),
            seal_recommend: false,
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
        if let Some(c) = &self.discord_channel {
            if !(15..=21).contains(&c.len()) || !c.bytes().all(|b| b.is_ascii_digit()) {
                bail!(
                    "threads.discord_channel {c:?} is not a Discord id (15-21 digits, as a string)"
                );
            }
        }
        if !self.import_day_usd.is_finite() || self.import_day_usd < 0.0 {
            bail!("threads.import_day_usd is not a dollar amount of 0 or more");
        }
        if self.import_profile.trim().is_empty() {
            bail!("threads.import_profile names no profile");
        }
        for (key, v) in [
            ("cross_recall_penalty", self.cross_recall_penalty),
            ("cross_recall_bar", self.cross_recall_bar),
        ] {
            if !(0.0..=1.0).contains(&v) {
                bail!("threads.{key} = {v} is outside 0 to 1");
            }
        }
        if self.sticky_half_life_secs == 0 {
            bail!("threads.sticky_half_life_secs is 0: a half-life is at least a second");
        }
        if self.ask_wait_secs == 0 {
            bail!("threads.ask_wait_secs is 0: a question waits at least a second");
        }
        if self.resume_keep_turns == 0 {
            bail!("threads.resume_keep_turns is 0: a resume keeps at least the newest turn");
        }
        if self.working_tokens < 2_000 {
            bail!("threads.working_tokens is under 2,000: a resume could not keep a turn");
        }
        if !(self.keep_warm_hours.is_finite() && self.keep_warm_hours >= 0.0) {
            bail!(
                "threads.keep_warm_hours must be 0 or more, not {}",
                self.keep_warm_hours
            );
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
        assert_eq!(
            (
                cfg.threads.sticky_half_life_secs,
                cfg.threads.route_wait_ms,
                cfg.threads.ask_wait_secs
            ),
            (600, 400, 900)
        );
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
        assert_eq!(cfg.threads.resume_keep_turns, 6);
        assert_eq!(cfg.threads.working_tokens, 60_000);
        for bad in ["resume_keep_turns = 0", "working_tokens = 10"] {
            assert!(crate::Config::parse(&format!("{secrets}\n[threads]\n{bad}\n")).is_err());
        }
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

    /// `keep_warm_hours` is 4 by default, 0 keeps no thread warm, and a
    /// negative window is refused (theseus-ytt8).
    #[test]
    fn the_keep_warm_window_is_four_hours_and_zero_is_none() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.threads.keep_warm_hours, 4.0);
        let off = crate::Config::from_toml("[threads]\nkeep_warm_hours = 0\n").unwrap();
        assert_eq!(off.threads.keep_warm_hours, 0.0);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let bad = format!("{secrets}\n[threads]\nkeep_warm_hours = -1\n");
        assert!(crate::Config::parse(&bad).is_err());
    }

    /// `discord_channel` is off by default, named in the template, takes a
    /// Discord id, and refuses anything else (theseus-sjzm).
    #[test]
    fn the_discord_channel_is_off_by_default_and_takes_an_id() {
        assert_eq!(ThreadsConfig::default().discord_channel, None);
        assert!(crate::Config::EXAMPLE_TOML.contains("# discord_channel = \""));
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let on = crate::Config::parse(&format!(
            "{secrets}\n[threads]\ndiscord_channel = \"123456789012345678\"\n"
        ))
        .unwrap();
        assert_eq!(
            on.0.threads.discord_channel.as_deref(),
            Some("123456789012345678")
        );
        for bad in ["general", "123", "1234567890123456789012"] {
            let t = format!("{secrets}\n[threads]\ndiscord_channel = \"{bad}\"\n");
            assert!(crate::Config::parse(&t).is_err(), "{bad}");
        }
    }
}
