//! `[threads]` (theseus-nhew): threads on virtual places `thread:<slug>`.
//! Every key has a default, so an owner's config names none of them unless
//! it differs. The rows built on the record (`thread-context`,
//! `thread-recall`, `thread-route`) add their keys here.
//!
//! - `enabled`: threads can be made and addressed (true). Off, `thread.new`
//!   and a `turn.submit` that names a thread are refused; the records stay.
//! - `max_live`: the most active and paused threads at once (50); a thread
//!   made past it is refused until one is done.
//! - `sticky_half_life_secs`: how fast a place's last thread stops being the
//!   likely one for its next message (600): the router's stickiness halves
//!   every this many seconds of gap (theseus-rha2).
//! - `route_wait_ms`: the most an unaddressed message waits for Jev's choice of
//!   thread, when the router's own score does not decide (400); on timeout the
//!   router's pick stands, tagged as a guess.
//! - `ask_wait_secs`: how long a message waits for the owner's answer to
//!   "which thread?" before it runs in the place's own conversation (900).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_live")]
    pub max_live: u32,
    #[serde(default = "default_half_life")]
    pub sticky_half_life_secs: u64,
    #[serde(default = "default_route_wait")]
    pub route_wait_ms: u64,
    #[serde(default = "default_ask_wait")]
    pub ask_wait_secs: u64,
}

fn default_enabled() -> bool {
    true
}
fn default_max_live() -> u32 {
    50
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

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            sticky_half_life_secs: default_half_life(),
            route_wait_ms: default_route_wait(),
            ask_wait_secs: default_ask_wait(),
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
        if self.sticky_half_life_secs == 0 {
            bail!("threads.sticky_half_life_secs is 0: a half-life is at least a second");
        }
        if self.ask_wait_secs == 0 {
            bail!("threads.ask_wait_secs is 0: a question waits at least a second");
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
        assert!(crate::Config::from_toml("[threads]\nbogus = 1\n").is_err());
    }
}
