//! `[sessions]` (theseus-emqx): the windows a session's state is derived
//! with, and when a session is re-titled. Every key has a default, so an
//! owner's config names none of them unless it differs.
//!
//! - `live_window_hours`: a session with a turn within it reads live (24).
//! - `empty_grace_minutes`: a session opened and never used reads retired
//!   (empty) this long after it opened (60), so one opened a moment ago
//!   never flashes retired.
//! - `retitle_within_turns`: the first prompt with substance among a
//!   session's first this many becomes its title, once, keeping the old
//!   one (3; 0 turns it off).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionsConfig {
    #[serde(default = "default_live_window_hours")]
    pub live_window_hours: f64,
    #[serde(default = "default_empty_grace_minutes")]
    pub empty_grace_minutes: f64,
    #[serde(default = "default_retitle_within_turns")]
    pub retitle_within_turns: u64,
}

fn default_live_window_hours() -> f64 {
    24.0
}
fn default_empty_grace_minutes() -> f64 {
    60.0
}
fn default_retitle_within_turns() -> u64 {
    3
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            live_window_hours: default_live_window_hours(),
            empty_grace_minutes: default_empty_grace_minutes(),
            retitle_within_turns: default_retitle_within_turns(),
        }
    }
}

impl SessionsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The rule `session.list` derives each state with.
    pub fn rule(&self) -> theseus_protocol::sessions::StateRule {
        theseus_protocol::sessions::StateRule {
            live_window_ms: (self.live_window_hours * 3_600_000.0) as u64,
            empty_grace_ms: (self.empty_grace_minutes * 60_000.0) as u64,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(self.live_window_hours > 0.0 && self.live_window_hours.is_finite()) {
            bail!(
                "[sessions] live_window_hours must be above 0, not {}",
                self.live_window_hours
            );
        }
        if !(self.empty_grace_minutes >= 0.0 && self.empty_grace_minutes.is_finite()) {
            bail!(
                "[sessions] empty_grace_minutes must be 0 or more, not {}",
                self.empty_grace_minutes
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<crate::Config> {
        let base = crate::Config::EXAMPLE_TOML;
        Ok(crate::Config::parse(&format!("{base}\n{toml}"))?.0)
    }

    /// The template's `[sessions]`, un-commented, is the defaults written
    /// out, and the template alone leaves the section at its defaults (the
    /// whole template's un-commenting test parses it too).
    #[test]
    fn the_templates_section_is_the_defaults() {
        let t = crate::Config::EXAMPLE_TOML;
        let at = t.find("# [sessions]").expect("the template's [sessions]");
        let section: String = t[at..]
            .lines()
            .take_while(|l| l.starts_with("# "))
            .map(|l| format!("{}\n", l.trim_start_matches("# ")))
            .collect();
        assert!(section.contains("retitle_within_turns"), "{section}");
        let cfg = parse(&section).unwrap();
        assert_eq!(cfg.sessions.live_window_hours, 24.0);
        assert_eq!(cfg.sessions.empty_grace_minutes, 60.0);
        assert_eq!(cfg.sessions.retitle_within_turns, 3);
        assert!(cfg.sessions.is_default());
        assert!(crate::Config::example().sessions.is_default());
    }

    #[test]
    fn the_rule_is_the_sections_windows_in_milliseconds() {
        let cfg = parse("[sessions]\nlive_window_hours = 2.5\nempty_grace_minutes = 0\n").unwrap();
        let rule = cfg.sessions.rule();
        assert_eq!(rule.live_window_ms, 9_000_000);
        assert_eq!(rule.empty_grace_ms, 0);
        assert_eq!(SessionsConfig::default().rule(), Default::default());
    }

    #[test]
    fn a_window_of_nothing_is_refused() {
        let e = format!(
            "{:#}",
            parse("[sessions]\nlive_window_hours = 0\n").unwrap_err()
        );
        assert!(e.contains("live_window_hours must be above 0"), "{e}");
        assert!(
            parse("[sessions]\nretitle_after = 3\n").is_err(),
            "an unknown key"
        );
    }
}
