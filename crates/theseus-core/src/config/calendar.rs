//! `[discord.calendar]` (theseus-4zc4d; horizon HZ11, owner decision D-H7):
//! the next days of reminders, routine runs and milestones as scheduled events
//! in one guild's Events tab. Off by default, and offered when it lands.
//!
//! A guild's Events tab is a calendar every Discord client already shows, with
//! Discord's own "starting now" notice for anyone marked Interested. The DM has
//! no events, so the calendar lives in a guild the bindings file trusts
//! (`private = true` on its `[[guild]]`): the binding checks that at each pass
//! and, where it is not so, writes nothing and says why in health.

use serde::{Deserialize, Serialize};

/// The most events one guild may hold at once (Discord's published limit is
/// 100 active and scheduled); `max_events` stays well under it so the guild's
/// own events keep their room.
pub const GUILD_EVENT_LIMIT: u32 = 100;
/// The most `max_events` may be set to.
pub const MAX_EVENTS: u32 = 50;
/// The longest window, in days.
pub const MAX_DAYS: u32 = 30;

/// `[discord.calendar]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarConfig {
    /// Mirror the agenda into the guild's Events tab. Off by default.
    #[serde(default)]
    pub enabled: bool,
    /// The guild's id, one the bindings file trusts (`private = true`).
    #[serde(default)]
    pub guild: String,
    /// How many days ahead the mirror looks.
    #[serde(default = "default_days")]
    pub days: u32,
    /// At most this many events stand at once; the rest wait their turn.
    #[serde(default = "default_max_events")]
    pub max_events: u32,
}

fn default_days() -> u32 {
    7
}
fn default_max_events() -> u32 {
    10
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            guild: String::new(),
            days: default_days(),
            max_events: default_max_events(),
        }
    }
}

impl CalendarConfig {
    /// Refuse a config that turns the calendar on without a place to put it,
    /// or sizes it past what Discord holds. A section that is off is not
    /// looked at, so a half-written one never stops a daemon.
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let id_ok =
            (15..=21).contains(&self.guild.len()) && self.guild.bytes().all(|c| c.is_ascii_digit());
        if !id_ok {
            anyhow::bail!(
                "discord.calendar: enabled needs `guild`, the id (15-21 digits, as a string) of a \
                 guild your bindings file trusts (`private = true`): the calendar's events are \
                 read by everyone in that guild, so it goes only where the owner is at home"
            );
        }
        if !(1..=MAX_DAYS).contains(&self.days) {
            anyhow::bail!(
                "discord.calendar: days is 1 to {MAX_DAYS}, not {}",
                self.days
            );
        }
        if !(1..=MAX_EVENTS).contains(&self.max_events) {
            anyhow::bail!(
                "discord.calendar: max_events is 1 to {MAX_EVENTS} (a guild holds {GUILD_EVENT_LIMIT} \
                 at most), not {}",
                self.max_events
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_and_asks_nothing_by_default() {
        let c = CalendarConfig::default();
        assert!(!c.enabled && c.guild.is_empty());
        assert_eq!((c.days, c.max_events), (7, 10));
        c.validate().expect("off needs nothing");
    }

    #[test]
    fn on_without_a_guild_is_refused_saying_why() {
        let c = CalendarConfig {
            enabled: true,
            ..Default::default()
        };
        let e = c.validate().unwrap_err().to_string();
        assert!(
            e.contains("needs `guild`") && e.contains("private = true"),
            "{e}"
        );
    }

    #[test]
    fn sizes_are_bounded() {
        let ok = CalendarConfig {
            enabled: true,
            guild: "100000000000000001".into(),
            ..Default::default()
        };
        ok.validate().unwrap();
        for (days, max) in [(0, 10), (31, 10), (7, 0), (7, 51)] {
            let c = CalendarConfig {
                days,
                max_events: max,
                ..ok.clone()
            };
            assert!(c.validate().is_err(), "{days} days, {max} events");
        }
    }
}
