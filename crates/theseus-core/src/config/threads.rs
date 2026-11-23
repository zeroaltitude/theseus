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
//!   request the turn waits for at most 400 ms: a note Jev reads as
//!   about the message's subject, not only the same kind of fact, is
//!   admitted with the penalty and labelled with its thread. Off, none is
//!   asked about and the drops stand.
//! - `sticky_half_life_secs`: how far back the place tail reaches before the
//!   place's last exchange (600; issue 43): a turn that runs in another session
//!   than that exchange is given what was said from this many seconds before it.
//! - `place_digest`, `place_digest_line_chars` and `route_wait_ms` are retired
//!   (2026-11-22): the place digest and the turn's wait for its `scope` answer
//!   are gone (`config/retired_keys.rs`).
//! - `working_set_grows` (true; theseus-98bz.5): after each turn that ran in a
//!   thread, the paths its calls touched (a file under the session's directory
//!   and its first component as a root) and the entities its message named join
//!   the thread's working set, but for what another live thread holds; the
//!   message's entity readers and Jev's thread inputs read it. Off, a set changes
//!   only by the owner's edit, a split or a merge.
//! - `bench_clock` (false; theseus-98bz.3): a test hook. On, a `turn.submit`
//!   carrying `route_at_ms` is placed at that clock (the place tail reads the
//!   bench's gaps) and is placed even when `ask -s` named its session; off,
//!   such a submit is refused. Off in every real config.
//! - `resume_keep_turns`: the newest turns a home keeps whole when its oldest settled stretch is
//!   measured (20; `turn::home_size_step`).
//! - `home_warn_percent`: the share of the serving model's context window a place's home may
//!   reach before the owner's DM is told once (50; 1 to 100). A long session is otherwise fine;
//!   the warning is for a home near the window.
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
//!
//! Retired on 2026-11-21, when the owner removed routing by a guess and the
//! thread importer: `ask_wait_secs`, `sticky_alone_asks`, `ask_every_thread`,
//! `unsure`, `single_offers_new`, `claim_home_turns`, `import_day_usd` and
//! `import_profile`. A config that sets one still loads: the key is read,
//! named once at startup ([`ThreadsConfig::retired`]), ignored and never
//! written back.

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
    #[serde(default = "default_half_life")]
    pub sticky_half_life_secs: u64,
    #[serde(default = "default_true")]
    pub working_set_grows: bool,
    #[serde(default)]
    pub bench_clock: bool,
    #[serde(default = "default_resume_keep_turns")]
    pub resume_keep_turns: u32,
    #[serde(default = "default_home_warn_percent")]
    pub home_warn_percent: u8,
    #[serde(default)]
    pub persona_engage: PersonaEngage,
    #[serde(default)]
    pub seal_recommend: bool,
    #[serde(default = "default_true")]
    pub link_proposals: bool,
    /// The retired keys (the module's last paragraph): read, warned of, ignored.
    #[serde(default, skip_serializing)]
    ask_wait_secs: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    sticky_alone_asks: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    ask_every_thread: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    unsure: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    single_offers_new: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    claim_home_turns: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    import_day_usd: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    import_profile: Option<toml::Value>,
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
fn default_penalty() -> f64 {
    0.35
}
fn default_bar() -> f64 {
    0.75
}
fn default_judge_k() -> usize {
    3
}
fn default_half_life() -> u64 {
    600
}
fn default_resume_keep_turns() -> u32 {
    20
}
fn default_home_warn_percent() -> u8 {
    50
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_live: default_max_live(),
            discord_channel: None,
            cross_recall_penalty: default_penalty(),
            cross_recall_bar: default_bar(),
            cross_recall_beats_own: true,
            cross_recall_needs_entity: true,
            base_distance: true,
            own_top_admissible: true,
            cross_recall_judge: true,
            cross_recall_judge_k: default_judge_k(),
            sticky_half_life_secs: default_half_life(),
            working_set_grows: true,
            bench_clock: false,
            resume_keep_turns: default_resume_keep_turns(),
            home_warn_percent: default_home_warn_percent(),
            persona_engage: PersonaEngage::default(),
            seal_recommend: false,
            link_proposals: true,
            ask_wait_secs: None,
            sticky_alone_asks: None,
            ask_every_thread: None,
            unsure: None,
            single_offers_new: None,
            claim_home_turns: None,
            import_day_usd: None,
            import_profile: None,
        }
    }
}

impl ThreadsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The retired keys this section sets, by the name the config gives them:
    /// what [`crate::Config::parse`] warns of, once.
    pub fn retired(&self) -> Vec<String> {
        let keys = [
            ("ask_wait_secs", &self.ask_wait_secs),
            ("sticky_alone_asks", &self.sticky_alone_asks),
            ("ask_every_thread", &self.ask_every_thread),
            ("unsure", &self.unsure),
            ("single_offers_new", &self.single_offers_new),
            ("claim_home_turns", &self.claim_home_turns),
            ("import_day_usd", &self.import_day_usd),
            ("import_profile", &self.import_profile),
        ];
        keys.iter()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| format!("threads.{k}"))
            .collect()
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
        if self.sticky_half_life_secs == 0 {
            bail!("threads.sticky_half_life_secs is 0: a half-life is at least a second");
        }
        if self.resume_keep_turns == 0 {
            bail!("threads.resume_keep_turns is 0: a resume keeps at least the newest turn");
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
        assert_eq!(cfg.threads.sticky_half_life_secs, 600);
        assert!(crate::Config::EXAMPLE_TOML.contains("# [threads]\n# enabled = true"));
        let off = crate::Config::from_toml("[threads]\nenabled = false\nmax_live = 3\n").unwrap();
        assert!(!off.threads.enabled && off.threads.max_live == 3);
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        assert!(crate::Config::parse(&format!("{secrets}\n[threads]\nmax_live = 0\n")).is_err());
        // The cold resume's sizes are retired (issue 44): any value loads, ignored and named.
        for retired in [
            "working_tokens = 10",
            "working_percent = 0",
            "working_percent = 101",
        ] {
            let (c, _) =
                crate::Config::parse(&format!("{secrets}\n[threads]\n{retired}\n")).unwrap();
            assert_eq!(c.ignored_keys.len(), 1, "{retired}");
        }
        assert!(crate::Config::from_toml("[threads]\nbogus = 1\n").is_err());
    }

    /// The place digest's keys are retired (2026-11-22): a settings file that
    /// still names one loads, each named among the ignored keys, and the
    /// template names none of them.
    #[test]
    fn the_place_digests_keys_are_retired_and_still_load() {
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        for retired in [
            "place_digest = false",
            "place_digest_line_chars = 10",
            "route_wait_ms = 400",
        ] {
            let (c, _) =
                crate::Config::parse(&format!("{secrets}\n[threads]\n{retired}\n")).unwrap();
            assert_eq!(c.ignored_keys.len(), 1, "{retired}");
        }
        for key in ["place_digest", "route_wait_ms"] {
            assert!(
                !crate::Config::EXAMPLE_TOML.contains(&format!("# {key} =")),
                "{key}"
            );
        }
    }

    /// A config that sets the keys retired on 2026-11-21 still loads (the
    /// owner's own sets `import_day_usd`): each is read and ignored, never
    /// written back, and startup names them in one plain line. The template
    /// names none of them.
    #[test]
    fn a_config_that_sets_a_retired_key_still_loads_and_is_told_once() {
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let old = format!(
            "{secrets}\n[threads]\nimport_day_usd = 0.5\nimport_profile = \"haiku\"\n\
             ask_wait_secs = 0\nsticky_alone_asks = false\nask_every_thread = false\n\
             unsure = \"ask\"\nsingle_offers_new = false\nclaim_home_turns = false\n"
        );
        let (cfg, warnings) = crate::Config::parse(&old).unwrap();
        let told: Vec<&String> = warnings.iter().filter(|w| w.contains("threads.")).collect();
        assert_eq!(told.len(), 1, "{warnings:?}");
        for key in [
            "import_day_usd",
            "import_profile",
            "ask_wait_secs",
            "sticky_alone_asks",
            "ask_every_thread",
            "unsure",
            "single_offers_new",
            "claim_home_turns",
        ] {
            assert!(
                told[0].contains(&format!("threads.{key}")),
                "{key}: {}",
                told[0]
            );
            assert!(
                !crate::Config::EXAMPLE_TOML.contains(&format!("# {key} =")),
                "the template still names {key}"
            );
        }
        let written = toml::to_string(&cfg.threads).unwrap();
        assert!(!written.contains("import_day_usd"), "{written}");
        let alone = format!("{secrets}\n[threads]\nimport_day_usd = 2.0\n");
        let (one, said) = crate::Config::parse(&alone).unwrap();
        assert_eq!(one.threads.retired(), ["threads.import_day_usd"]);
        assert!(said.iter().any(|w| w.contains("remove it")), "{said:?}");
        let (plain, quiet) =
            crate::Config::parse(&format!("{secrets}\n[threads]\nmax_live = 3\n")).unwrap();
        assert!(plain.threads.retired().is_empty());
        assert!(quiet.iter().all(|w| !w.contains("threads.")), "{quiet:?}");
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
}
