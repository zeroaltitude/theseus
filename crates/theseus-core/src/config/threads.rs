//! `[threads]` (theseus-nhew): threads on virtual places `thread:<slug>`.
//! Every key has a default, so an owner's config names none of them unless
//! it differs. The rows built on the record (`thread-context`,
//! `thread-recall`, `thread-route`) add their keys here.
//!
//! - `enabled`: threads can be made and addressed (true). Off, `thread.new`
//!   and a `turn.submit` that names a thread are refused; the records stay.
//! - `max_live`: the most active and paused threads at once (50); a thread
//!   made past it is refused until one is done.
//! - `resume_keep_turns`: the newest turns a thread keeps whole when its next
//!   message finds its cache gone (6; `thread-context`).
//! - `working_tokens`: what a thread's request stays within after such a cold
//!   resume (60,000), its newest turn aside.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_live")]
    pub max_live: u32,
    #[serde(default = "default_resume_keep_turns")]
    pub resume_keep_turns: u32,
    #[serde(default = "default_working_tokens")]
    pub working_tokens: u64,
}

fn default_enabled() -> bool {
    true
}
fn default_max_live() -> u32 {
    50
}
fn default_resume_keep_turns() -> u32 {
    6
}
fn default_working_tokens() -> u64 {
    60_000
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            resume_keep_turns: default_resume_keep_turns(),
            working_tokens: default_working_tokens(),
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
        if self.resume_keep_turns == 0 {
            bail!("threads.resume_keep_turns is 0: a resume keeps at least the newest turn");
        }
        if self.working_tokens < 2_000 {
            bail!("threads.working_tokens is under 2,000: a resume could not keep a turn");
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
        assert_eq!(cfg.threads.resume_keep_turns, 6);
        assert_eq!(cfg.threads.working_tokens, 60_000);
        for bad in ["resume_keep_turns = 0", "working_tokens = 10"] {
            assert!(crate::Config::parse(&format!("{secrets}\n[threads]\n{bad}\n")).is_err());
        }
        assert!(crate::Config::from_toml("[threads]\nbogus = 1\n").is_err());
    }
}
