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

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_live")]
    pub max_live: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord_channel: Option<String>,
}

fn default_enabled() -> bool {
    true
}
fn default_max_live() -> u32 {
    50
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            discord_channel: None,
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
