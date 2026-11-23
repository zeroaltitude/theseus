//! Historical thread settings; persona engagement remains active.
//! `enabled` is retired through the loader table; old configurations still load.
//! - `max_live` is retired (2026-11-22, `config/retired_keys.rs`): nothing
//!   counts live threads since threads are no longer containers.
//! - `place_digest`, `place_digest_line_chars` and `route_wait_ms` are retired
//!   (2026-11-22): the place digest and the turn's wait for its `scope` answer
//!   are gone (`config/retired_keys.rs`).
//! - `persona_engage` (`shadow`): whether Jev's `persona` verdict (route.v5,
//!   theseus-c1uz) engages a persona. An explicit address (a name at the
//!   message's start, `@name`, `/as name`) always does; `shadow` records
//!   Jev's pick and keeps the prior, `act` engages it at 0.90 and shows
//!   a why from 0.60. The maintainer sets `act` after the persona holdout.
//!
//! Retired on 2026-11-21, when the owner removed routing by a guess and the
//! thread importer: `ask_wait_secs`, `sticky_alone_asks`, `ask_every_thread`,
//! `unsure`, `single_offers_new`, `claim_home_turns`, `import_day_usd` and
//! `import_profile`. A config that sets one still loads: the key is read,
//! named once at startup ([`ThreadsConfig::retired`]), ignored and never
//! written back.
//!
//! Retired on 2026-11-22 (threads are no longer containers), through the
//! loader's table (`config/retired_keys.rs`), which takes each out before the
//! section is read and says so once: `cross_recall_penalty`, `cross_recall_bar`,
//! `cross_recall_beats_own`, `cross_recall_needs_entity`, `base_distance`,
//! `own_top_admissible`, `cross_recall_judge`, `cross_recall_judge_k`,
//! `working_set_grows`, `bench_clock`, `resume_keep_turns`, `home_warn_percent`,
//! `seal_recommend`; and, with the link offer and the Discord thread mirror
//! that read them, `link_proposals`, `discord_channel` and
//! `sticky_half_life_secs` (XD3).

use anyhow::Result;
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsConfig {
    #[serde(default)]
    pub persona_engage: PersonaEngage,
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
        let section = crate::Config::EXAMPLE_TOML
            .split("# [threads]\n")
            .nth(1)
            .unwrap()
            .split("# ----------------------------------------------------------------")
            .next()
            .unwrap();
        assert!(section.contains("# persona_engage = \"shadow\""));
        assert!(!section.contains("# enabled ="));
        let off = crate::Config::from_toml("[threads]\nenabled = false\n").unwrap();
        assert_eq!(off.threads, ThreadsConfig::default());
        assert!(off
            .ignored_keys
            .iter()
            .any(|key| key.name == "threads.enabled"));
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
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
            crate::Config::parse(&format!("{secrets}\n[threads]\nenabled = true\n")).unwrap();
        assert!(
            plain.threads.retired().is_empty(),
            "the loader removes table-retired keys before this section is read"
        );
        assert!(
            quiet.iter().any(|w| w.contains("threads.enabled")),
            "{quiet:?}"
        );
    }

    /// The three inert keys retired by XD3 (`discord_channel`, `sticky_half_life_secs`,
    /// `link_proposals`): an old config that sets them still loads, even with a value the
    /// field once refused, and each is named once and ignored; the template names none.
    #[test]
    fn the_inert_thread_keys_still_load_and_are_named() {
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let old = format!(
            "{secrets}\n[threads]\ndiscord_channel = \"general\"\n\
             sticky_half_life_secs = 0\nlink_proposals = false\n"
        );
        let (cfg, _) = crate::Config::parse(&old).unwrap();
        let names: Vec<&str> = cfg.ignored_keys.iter().map(|i| i.name.as_str()).collect();
        for key in ["discord_channel", "sticky_half_life_secs", "link_proposals"] {
            assert!(
                names.contains(&format!("threads.{key}").as_str()),
                "{key}: {names:?}"
            );
            assert!(
                !crate::Config::EXAMPLE_TOML.contains(&format!("# {key} =")),
                "the template still names {key}"
            );
        }
    }
}

#[test]
fn thread_enable_is_retired_without_retiring_persona_engagement() {
    for enabled in [true, false] {
        let text = format!("[threads]\nenabled = {enabled}\npersona_engage = 'act'\n");
        let cfg = crate::Config::from_toml(&text).unwrap();
        assert!(cfg
            .ignored_keys
            .iter()
            .any(|key| key.name == "threads.enabled"));
        assert_eq!(cfg.threads.persona_engage, PersonaEngage::Act);
        let serialized = toml::to_string(&cfg.threads).unwrap();
        assert!(!serialized.contains("enabled"));
    }
}
