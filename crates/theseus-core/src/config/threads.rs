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
//! - `own_top_admissible` (true; theseus-98bz.8, theseus-stwkb): the asking
//!   thread's own best hit that a note no thread claims (the place's own
//!   conversation) must outrank (`cross_recall_beats_own`) is its best not
//!   already in the turn's context, so a home note holding the one fact the
//!   thread lacks is not kept out by a note it has. Another live thread's note
//!   must still outrank its best hit in context or not: a thread's own hits
//!   are mostly in its context, and the bar held other streams out by them.
//!   Off, every own hit counts for every note, in context or not.
//! - `cross_recall_judge` (true; theseus-98bz.10): the notes the distance
//!   dropped whose score is above the bar go to Jev (`recall_cross.v1`), at
//!   most `cross_recall_judge_k` (3, 1 to 10) of them, best first, in one
//!   request the turn waits for at most `route_wait_ms`: a note Jev reads as
//!   about the message's subject, not only the same kind of fact, is
//!   admitted with the penalty and labelled with its thread. Off, none is
//!   asked about and the drops stand.
//! - `place_digest` (true; theseus-98bz.9): when Jev's `scope` answer
//!   (classify.v4) says a message in a thread asks across everything going on,
//!   that turn's request gains a digest of the place's other live threads, one
//!   labelled line each, a sealed or seal-pending one only counted. Off, no
//!   turn waits for the answer and no request carries a digest.
//! - `place_digest_line_chars` (200): the longest a digest's line is.
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
//! - `working_set_grows` (true; theseus-98bz.5): after each turn that ran in a
//!   thread, the paths its calls touched (a file under the session's directory
//!   and its first component as a root) and the entities its message named join
//!   the thread's working set, but for what another live thread holds; the
//!   router's entity hits and Jev's thread inputs read it. Off, a set changes
//!   only by the owner's edit, a split or a merge.
//! - `bench_clock` (false; theseus-98bz.3): a test hook. On, a `turn.submit`
//!   carrying `route_at_ms` is routed at that clock (stickiness and the reply
//!   chain read the bench's gaps) and is placed even when `ask -s` named its
//!   session; off, such a submit is refused. Off in every real config.
//! - `single_offers_new` (true; theseus-98bz.7): in a place with one live
//!   thread, a message the thread scores below the confirm bar that names two
//!   or more entities no working set holds is asked about (the thread, a new
//!   one, or home), as a place with two would; a spoken turn still runs at
//!   home. Off, it runs at home unasked, as before.
//! - `resume_keep_turns`: the newest turns a thread keeps whole when its next
//!   message finds its cache gone (20; `thread-context`).
//! - `working_percent`: what a thread's request stays within after such a cold
//!   resume, as a share of the serving model's context window (40; 1 to 100), its newest
//!   turn aside. `working_tokens`, when set, is that size in tokens and wins.
//! - `home_warn_percent`: the share of the serving model's context window a place's home may
//!   reach before the owner's DM is told once (50; 1 to 100). A long session is otherwise fine;
//!   the warning is for a home near the window. Not `working_tokens`.
//! - `persona_engage` (`shadow`): whether Jev's `persona` verdict (route.v5,
//!   theseus-c1uz) engages a persona. An explicit address (a name at the
//!   message's start, `@name`, `/as name`) always does; `shadow` records
//!   Jev's pick and keeps the prior, `act` engages it at 0.90 and shows
//!   a why from 0.60. The maintainer sets `act` after the persona holdout.
//! - `seal_recommend` (false): whether a `yes` of route.v5's `seal` question
//!   at the confirm bar or above, in a thread, proposes sealing it (a card to
//!   the owner's DM; Jev recommends, the owner seals). Off until its holdout.
//! - `link_proposals` (true; theseus-98bz.11): when Jev admits notes of another live, unsealed thread
//!   into a thread's recall in two recalls, one sentence to the owner's DM offers `theseus thread link`
//!   (once a pair, whatever the owner does). It links nothing itself. Off, nothing is counted, written
//!   or posted.
//! - `claim_home_turns` (true; theseus-98bz.6): a turn that ran at a place's home (asked and answered
//!   `home`, an ask nobody answered, the router's home pick) whose written files all fall in one live,
//!   unsealed thread's working set becomes that thread's memory: its nodes are keyed to the thread
//!   (`moved.of.<node>`, a `claim_` value), so the thread's recall reads them as its own.
//!   `theseus thread unclaim ID` undoes one. Off, nothing is claimed, written or counted.

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
    #[serde(default)]
    pub import_profile: Option<String>,
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
    #[serde(default = "default_true")]
    pub own_top_admissible: bool,
    #[serde(default = "default_true")]
    pub cross_recall_judge: bool,
    #[serde(default = "default_judge_k")]
    pub cross_recall_judge_k: usize,
    #[serde(default = "default_true")]
    pub place_digest: bool,
    #[serde(default = "default_digest_line_chars")]
    pub place_digest_line_chars: usize,
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
    #[serde(default = "default_true")]
    pub working_set_grows: bool,
    #[serde(default)]
    pub bench_clock: bool,
    #[serde(default = "default_true")]
    pub single_offers_new: bool,
    #[serde(default = "default_resume_keep_turns")]
    pub resume_keep_turns: u32,
    #[serde(default)]
    pub working_tokens: Option<u64>,
    #[serde(default = "default_working_percent")]
    pub working_percent: u8,
    #[serde(default = "default_home_warn_percent")]
    pub home_warn_percent: u8,
    #[serde(default)]
    pub persona_engage: PersonaEngage,
    #[serde(default)]
    pub seal_recommend: bool,
    #[serde(default = "default_true")]
    pub link_proposals: bool,
    #[serde(default = "default_true")]
    pub claim_home_turns: bool,
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
fn default_penalty() -> f64 {
    0.35
}
fn default_bar() -> f64 {
    0.75
}
fn default_judge_k() -> usize {
    3
}
fn default_digest_line_chars() -> usize {
    200
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
    20
}
fn default_working_percent() -> u8 {
    40
}
fn default_home_warn_percent() -> u8 {
    50
}

impl ThreadsConfig {
    /// The tokens a cold resume stays within on a model with `window`: `working_tokens` when
    /// set, else `working_percent` of the window.
    pub fn working_size(&self, window: u64) -> u64 {
        self.working_tokens
            .unwrap_or_else(|| window / 100 * u64::from(self.working_percent))
    }
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            discord_channel: None,
            import_day_usd: default_import_day_usd(),
            import_profile: None,
            cross_recall_penalty: default_penalty(),
            cross_recall_bar: default_bar(),
            cross_recall_beats_own: true,
            cross_recall_needs_entity: true,
            base_distance: true,
            own_top_admissible: true,
            cross_recall_judge: true,
            cross_recall_judge_k: default_judge_k(),
            place_digest: true,
            place_digest_line_chars: default_digest_line_chars(),
            sticky_half_life_secs: default_half_life(),
            route_wait_ms: default_route_wait(),
            ask_wait_secs: default_ask_wait(),
            sticky_alone_asks: true,
            ask_every_thread: true,
            working_set_grows: true,
            bench_clock: false,
            single_offers_new: true,
            resume_keep_turns: default_resume_keep_turns(),
            working_tokens: None,
            working_percent: default_working_percent(),
            home_warn_percent: default_home_warn_percent(),
            persona_engage: PersonaEngage::default(),
            seal_recommend: false,
            link_proposals: true,
            claim_home_turns: true,
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
        if self
            .import_profile
            .as_deref()
            .is_some_and(|p| p.trim().is_empty())
        {
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
        if !(1..=10).contains(&self.cross_recall_judge_k) {
            bail!(
                "threads.cross_recall_judge_k is {}: Jev reads 1 to 10 notes a request",
                self.cross_recall_judge_k
            );
        }
        if self.place_digest_line_chars < 40 {
            bail!("threads.place_digest_line_chars is under 40: a line could not name its thread");
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
        if self.working_tokens.is_some_and(|w| w < 2_000) {
            bail!("threads.working_tokens is under 2,000: a resume could not keep a turn");
        }
        if !(1..=100).contains(&self.working_percent) {
            bail!(
                "threads.working_percent is {}: a share of the model's window, 1 to 100 (default 40)",
                self.working_percent
            );
        }
        if !(1..=100).contains(&self.home_warn_percent) {
            bail!(
                "threads.home_warn_percent is {}: a share of the model's window, 1 to 100 (default 50)",
                self.home_warn_percent
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
        assert_eq!(cfg.threads.import_profile, None);
        assert!(crate::Config::EXAMPLE_TOML.contains("# import_day_usd = 2.0"));
        let pricey = crate::Config::from_toml("[threads]\nimport_day_usd = 0.5\n").unwrap();
        assert_eq!(pricey.threads.import_day_usd, 0.5);
        for bad in ["import_day_usd = -1.0", "import_profile = \"\""] {
            let toml = format!("{secrets}\n[threads]\n{bad}\n");
            assert!(crate::Config::parse(&toml).is_err(), "{bad}");
        }
        assert_eq!(cfg.threads.resume_keep_turns, 20);
        assert_eq!(cfg.threads.working_tokens, None);
        assert_eq!(cfg.threads.working_percent, 40);
        for bad in [
            "resume_keep_turns = 0",
            "working_tokens = 10",
            "working_percent = 0",
            "working_percent = 101",
        ] {
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
        assert!(cfg.threads.cross_recall_needs_entity && cfg.threads.own_top_admissible);
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_needs_entity = true"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# own_top_admissible = true"));
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

    /// Jev's read of the near misses (theseus-98bz.10) is on, three notes a
    /// request, named in the template; it turns off, and a k outside 1 to 10
    /// is refused.
    #[test]
    fn the_cross_judge_reads_three_and_turns_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.cross_recall_judge);
        assert_eq!(cfg.threads.cross_recall_judge_k, 3);
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_judge = true"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# cross_recall_judge_k = 3"));
        let off = crate::Config::from_toml("[threads]\ncross_recall_judge = false\n").unwrap();
        assert!(!off.threads.cross_recall_judge);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        for k in [0, 11] {
            let bad = format!("{secrets}\n[threads]\ncross_recall_judge_k = {k}\n");
            assert!(crate::Config::parse(&bad).is_err(), "{k}");
        }
    }

    /// The place digest (theseus-98bz.9) is on by default, named in the
    /// template with its line cap, turns off, and refuses a cap too short to
    /// name a thread.
    #[test]
    fn the_place_digest_is_on_by_default_and_turns_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.place_digest);
        assert_eq!(cfg.threads.place_digest_line_chars, 200);
        assert!(crate::Config::EXAMPLE_TOML.contains("# place_digest = true"));
        assert!(crate::Config::EXAMPLE_TOML.contains("# place_digest_line_chars = 200"));
        let off = crate::Config::from_toml("[threads]\nplace_digest = false\n").unwrap();
        assert!(!off.threads.place_digest);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let bad = format!("{secrets}\n[threads]\nplace_digest_line_chars = 10\n");
        assert!(crate::Config::parse(&bad).is_err());
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

    /// Link proposals (theseus-98bz.11) are on by default, named in the template, and turn off.
    #[test]
    fn link_proposals_are_on_by_default_and_turn_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.link_proposals && ThreadsConfig::default().link_proposals);
        assert!(crate::Config::EXAMPLE_TOML.contains("# link_proposals = true"));
        let off = crate::Config::from_toml("[threads]\nlink_proposals = false\n").unwrap();
        assert!(!off.threads.link_proposals);
    }

    /// `home_warn_percent` is 50 by default, named in the template, read back, and refused
    /// outside 1 to 100.
    #[test]
    fn the_home_warning_is_a_share_of_the_window() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.threads.home_warn_percent, 50);
        assert!(crate::Config::EXAMPLE_TOML.contains("# home_warn_percent = 50"));
        let set = crate::Config::from_toml("[threads]\nhome_warn_percent = 80\n").unwrap();
        assert_eq!(set.threads.home_warn_percent, 80);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        for bad in [0, 101] {
            let note = format!("{secrets}\n[threads]\nhome_warn_percent = {bad}\n");
            let e = crate::Config::parse(&note).unwrap_err().to_string();
            assert!(e.contains("home_warn_percent"), "{bad}: {e}");
        }
    }

    /// A cold resume's working size is a share of the serving model's window; a set
    /// `working_tokens` wins (theseus-d9je7).
    #[test]
    fn the_working_size_is_a_share_of_the_window() {
        let cfg = ThreadsConfig::default();
        assert_eq!(cfg.working_size(1_000_000), 400_000);
        assert_eq!(cfg.working_size(200_000), 80_000);
        let set = crate::Config::from_toml("[threads]\nworking_tokens = 60000\n").unwrap();
        assert_eq!(set.threads.working_size(1_000_000), 60_000);
        let pct = crate::Config::from_toml("[threads]\nworking_percent = 25\n").unwrap();
        assert_eq!(pct.threads.working_size(200_000), 50_000);
        assert!(crate::Config::EXAMPLE_TOML.contains("# working_percent = 40"));
    }

    /// A thread's set grows from its turns (theseus-98bz.5) by default, named
    /// in the template, and the switch turns it off.
    #[test]
    fn the_working_set_grows_by_default_and_turns_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.working_set_grows && ThreadsConfig::default().working_set_grows);
        assert!(crate::Config::EXAMPLE_TOML.contains("# working_set_grows = true"));
        let off = crate::Config::from_toml("[threads]\nworking_set_grows = false\n").unwrap();
        assert!(!off.threads.working_set_grows);
    }

    /// The bench's clock (theseus-98bz.3) is off by default, named in the
    /// template, and a test turns it on.
    #[test]
    fn the_bench_clock_is_off_by_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(!cfg.threads.bench_clock && !ThreadsConfig::default().bench_clock);
        assert!(crate::Config::EXAMPLE_TOML.contains("# bench_clock = false"));
        let on = crate::Config::from_toml("[threads]\nbench_clock = true\n").unwrap();
        assert!(on.threads.bench_clock);
    }

    /// A lone thread offers a new one (theseus-98bz.7) by default, named in the
    /// template, and the switch turns it off.
    #[test]
    fn a_lone_thread_offers_a_new_one_by_default_and_turns_off() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.threads.single_offers_new && ThreadsConfig::default().single_offers_new);
        assert!(crate::Config::EXAMPLE_TOML.contains("# single_offers_new = true"));
        let off = crate::Config::from_toml("[threads]\nsingle_offers_new = false\n").unwrap();
        assert!(!off.threads.single_offers_new);
    }
}
