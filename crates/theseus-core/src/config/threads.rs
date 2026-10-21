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
//! - `cross_recall_beats_own` (true; theseus-wncdv): ...and, once it has paid,
//!   another thread's candidate must still outrank the asking thread's own best
//!   hit of the query. Streams share the shape of their facts (a port, a branch,
//!   a ticket), so a question about this thread's port scores another's port
//!   nearly as high; with this off the bar alone decides, and the threadbench
//!   answered about other streams' facts on 4 to 7 of 12 cross-stream questions.
//! - `cross_recall_needs_entity` (true; theseus-wncdv): ...and a candidate whose
//!   chunk holds concrete entities (a path, a ticket, a host) of which the
//!   message names none is dropped: the question's shape alone is no reason to
//!   pull another stream's note. A chunk with no entities is judged by the bar
//!   and the thread's own best hit alone.
//! - `base_distance` (true; theseus-wncdv): a candidate no thread claims (the
//!   place's own conversation, where each stream's first messages run before its
//!   thread exists) is another stream's to a threaded asker: it pays the same
//!   distance and a name never waives it. Off, it recalls as before. A node a
//!   split moved belongs to the thread it moved to, whichever session it is in.
//! - `sticky_half_life_secs`: how fast a place's last thread stops being the
//!   likely one for its next message (600): the router's stickiness halves
//!   every this many seconds of gap (theseus-rha2).
//! - `route_wait_ms`: the most an unaddressed message waits for Jev's choice of
//!   thread, when the router's own score does not decide (400); on timeout the
//!   router's pick stands, tagged as a guess.
//! - `ask_wait_secs`: how long a message waits for the owner's answer to
//!   "which thread?" before it runs in the place's own conversation (900).
//! - `sticky_alone_asks` (true; theseus-eqp1v): a thread that stands on
//!   stickiness and the reply chain alone (no entity of its named, no learned
//!   correction) cannot overrule Jev's other pick: the message is asked about,
//!   and the ask offers a new thread when the message names two or more
//!   entities no live thread holds. Off, the decision table is as before.
//! - `ask_every_thread` (true; theseus-98bz.4): "which thread?" lists every
//!   live, unsealed thread (four at most; past four, the best three and
//!   `other`), and any live thread's slug answers it. Off, it lists the best
//!   two, as before.
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
    #[serde(default = "default_true")]
    pub cross_recall_beats_own: bool,
    #[serde(default = "default_true")]
    pub cross_recall_needs_entity: bool,
    #[serde(default = "default_true")]
    pub base_distance: bool,
    #[serde(default = "default_half_life")]
    pub sticky_half_life_secs: u64,
    #[serde(default = "default_route_wait")]
    pub route_wait_ms: u64,
    #[serde(default = "default_ask_wait")]
    pub ask_wait_secs: u64,
    #[serde(default = "default_true")]
    pub sticky_alone_asks: bool,
    #[serde(default = "default_true")]
    pub ask_every_thread: bool,
    #[serde(default = "default_resume_keep_turns")]
    pub resume_keep_turns: u32,
    #[serde(default = "default_working_tokens")]
    pub working_tokens: u64,
    #[serde(default)]
    pub persona_engage: PersonaEngage,
    #[serde(default)]
    pub seal_recommend: bool,
}

fn default_enabled() -> bool {
    true
}
fn default_true() -> bool {
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
            cross_recall_beats_own: true,
            cross_recall_needs_entity: true,
            base_distance: true,
            sticky_half_life_secs: default_half_life(),
            route_wait_ms: default_route_wait(),
            ask_wait_secs: default_ask_wait(),
            sticky_alone_asks: true,
            ask_every_thread: true,
            resume_keep_turns: default_resume_keep_turns(),
            working_tokens: default_working_tokens(),
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

    /// The streams-do-not-mix keys (theseus-wncdv) are on by default, named in the
    /// template, and each can be turned off.
    #[test]
    fn the_streams_do_not_mix_keys_are_on_by_default_and_each_turns_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.cross_recall_beats_own && cfg.threads.base_distance);
        assert!(cfg.threads.cross_recall_needs_entity);
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_needs_entity = true"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_beats_own = true"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# base_distance = true"));
        let loose = crate::Config::from_toml(
            "[threads]\ncross_recall_beats_own = false\nbase_distance = false\n\
             cross_recall_needs_entity = false\n",
        )
        .unwrap();
        assert!(!loose.threads.cross_recall_beats_own && !loose.threads.base_distance);
        assert!(!loose.threads.cross_recall_needs_entity);
    }

    /// The router's rules (theseus-eqp1v) are on by default, named in the
    /// template, and each turns off.
    #[test]
    fn the_routers_rules_are_on_by_default_and_each_turns_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.sticky_alone_asks && cfg.threads.ask_every_thread);
        assert!(crate::Config::EXAMPLE_TOML.contains("# sticky_alone_asks = true"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# ask_every_thread = true"));
        let off = crate::Config::from_toml(
            "[threads]\nsticky_alone_asks = false\nask_every_thread = false\n",
        )
        .unwrap();
        assert!(!off.threads.sticky_alone_asks && !off.threads.ask_every_thread);
    }

    /// `[cache] threads_keep_warm_hours` is 4 by default, 0 keeps no thread warm, and a
    /// negative window is refused (theseus-ytt8).
    #[test]
    fn the_keep_warm_window_is_four_hours_and_zero_is_none() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.cache.threads_keep_warm_hours, 4.0);
        let off = crate::Config::from_toml("[cache]\nthreads_keep_warm_hours = 0\n").unwrap();
        assert_eq!(off.cache.threads_keep_warm_hours, 0.0);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let bad = format!("{secrets}\n[cache]\nthreads_keep_warm_hours = -1\n");
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
