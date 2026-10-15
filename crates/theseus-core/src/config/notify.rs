//! `[notify]` (theseus-j7cje): when an open question reaches the owner's
//! phone. A question shows at once, silently, on every Theseus screen and on
//! the owner's Discord DM board. If it is still open after `escalate_after_secs`
//! with no Theseus screen open, or after `escalate_with_screen_secs` with one,
//! the core posts one "needs you" to the owner's DM, which pings. The pacing
//! of unprompted posts (the owner's rule of one per 30 minutes) is a later
//! row's. Every key has a default, so an owner's config names none unless it
//! differs.
//!
//! - `escalate_after_secs`: how long a question waits with no screen open (60).
//! - `escalate_with_screen_secs`: how long it waits while a screen is open (300).
//! - `unprompted_pace_secs`: the one pacing rule for posts nobody asked for
//!   (`pacer.rs`): at most one in this long (1,800, thirty minutes), what is
//!   due meanwhile going out together in the next. 0: no rule.
//! - An `owner_ask` names its own life, so none is here.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifyConfig {
    #[serde(default = "default_after")]
    pub escalate_after_secs: u64,
    #[serde(default = "default_with_screen")]
    pub escalate_with_screen_secs: u64,
    #[serde(default = "default_pace")]
    pub unprompted_pace_secs: u64,
}

fn default_after() -> u64 {
    60
}
fn default_with_screen() -> u64 {
    300
}
fn default_pace() -> u64 {
    1800
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            escalate_after_secs: default_after(),
            escalate_with_screen_secs: default_with_screen(),
            unprompted_pace_secs: default_pace(),
        }
    }
}

impl NotifyConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if self.escalate_after_secs == 0 || self.escalate_with_screen_secs == 0 {
            bail!("notify.escalate_after_secs and escalate_with_screen_secs must be at least 1");
        }
        if self.escalate_with_screen_secs < self.escalate_after_secs {
            bail!(
                "notify.escalate_with_screen_secs ({}) is shorter than escalate_after_secs ({}): \
                 a screen being open only ever waits longer",
                self.escalate_with_screen_secs,
                self.escalate_after_secs
            );
        }
        Ok(())
    }

    /// How long a question waits before it pings, given whether a screen is
    /// open, in milliseconds.
    pub fn wait_ms(&self, screen_open: bool) -> u64 {
        1000 * match screen_open {
            true => self.escalate_with_screen_secs,
            false => self.escalate_after_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[notify]` is the default, and a note may change it.
    #[test]
    fn the_templates_notify_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.notify, NotifyConfig::default());
        assert_eq!(cfg.notify.wait_ms(false), 60_000);
        assert_eq!(cfg.notify.wait_ms(true), 300_000);
        assert!(crate::Config::EXAMPLE_TOML.contains("# [notify]\n# escalate_after_secs = 60"));
        let own = crate::Config::from_toml("[notify]\nescalate_after_secs = 5\n").unwrap();
        assert_eq!(own.notify.escalate_after_secs, 5);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        assert!(
            crate::Config::parse(&format!("{secrets}\n[notify]\nescalate_after_secs = 0\n"))
                .is_err()
        );
        assert!(crate::Config::parse(&format!(
            "{secrets}\n[notify]\nescalate_after_secs = 90\nescalate_with_screen_secs = 30\n"
        ))
        .is_err());
        assert!(crate::Config::from_toml("[notify]\nbogus = 1\n").is_err());
    }
}
