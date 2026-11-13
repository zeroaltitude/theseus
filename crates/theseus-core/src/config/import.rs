//! `[import.openclaw]` (theseus-pjmj7): the nightly OpenClaw sync's own
//! settings. The sync itself is `theseus import sync`, which a systemd user
//! timer runs (`scripts/user-service.sh sync-timer`); the daemon only holds
//! these settings and the cursor, and answers `import.sync.plan`.
//!
//! - `state_dir`: OpenClaw's state directory (the one holding `agents/`).
//!   Unset, the sync is off: the public template leaves it unset and the
//!   owner's own config names it.
//! - `owner_ids`: the owner's sender ids in OpenClaw's channels. An inbound
//!   message from one is the owner's own (`operator`); any other sender's is
//!   outside text. They are the owner's, so they live in the owner's config,
//!   never in this repository.
//! - `agents`: the agents to read; none is every agent under `agents/`.
//! - `sync_deadline_mins`: the wall clock the daemon's half of one run may
//!   take before it stops where it is and returns what it has
//!   (`rpc/import_sync.rs`). 90 by default. It bounds the DAEMON, where the
//!   unit's `TimeoutStartSec=` (`scripts/user-service.sh`, 2 h) bounds only
//!   the client, so it must stay well under that or it could never fire: the
//!   range refuses anything over `MAX_SYNC_DEADLINE_MINS`.
//!
//! `[import.notes]` (`b4-theseus-holds-its-own-notes`): the agent-notes
//! import's two switches, both **off by default**, read by
//! `crate::import::notes::Gate` at `import.episodes`.
//!
//! - `enabled`: whether an `agent-notes` episode is written at all. Off, each
//!   one is refused and the refusal names what the durable-versus-run-state
//!   rule read it as, so the off state IS the dry run: the import runs, every
//!   note comes back classified, and nothing is written. It is what the bulk
//!   pass waits on, until the owner has seen the rule
//!   (`docs/design/agent-notes-rule.md`).
//! - `keep_run_state`: whether a note the rule reads as run state is written
//!   once the path is on. Off, it is refused and named; the durable ones go
//!   in. Both default off because this import widens what Theseus holds to
//!   our own operating notes.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenclawSyncConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    /// The wall clock one run's daemon half may take, in minutes.
    #[serde(default = "default_sync_deadline_mins")]
    pub sync_deadline_mins: u64,
}

/// The cap a config that says nothing gets: 90 minutes.
///
/// Measured rather than round (theseus-ectzg). The runs on record took 5 to
/// 24 minutes, the slowest run ever observed to FINISH took about 85, and
/// 2026-11-12's took 7 min 25 s, so 90 is above every run known to work --
/// and half an hour under the unit's 2 h, which is room enough for the run to
/// stop, return, and have the client print its summary.
pub fn default_sync_deadline_mins() -> u64 {
    90
}

/// The largest bound the loader takes.
///
/// The cap must stay strictly below the sync unit's `TimeoutStartSec=` (2 h,
/// `scripts/user-service.sh`) or it can never fire: the client would be
/// killed first and the daemon would carry on, and the next night's run would
/// begin beside a pass still going. 115 leaves five minutes for the run to
/// return and the client to print what it did.
pub const MAX_SYNC_DEADLINE_MINS: u64 = 115;

impl Default for OpenclawSyncConfig {
    fn default() -> Self {
        Self {
            state_dir: None,
            owner_ids: Vec::new(),
            agents: Vec::new(),
            sync_deadline_mins: default_sync_deadline_mins(),
        }
    }
}

/// `[import.notes]`: the agent-notes import's switches
/// (`b4-theseus-holds-its-own-notes`). Both off by default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotesImportConfig {
    /// Whether an `agent-notes` episode is written at all. Off, every one is
    /// refused with its reading: the dry run the owner is shown.
    #[serde(default)]
    pub enabled: bool,
    /// Whether a note the rule reads as run state is written, once the path
    /// is on.
    #[serde(default)]
    pub keep_run_state: bool,
}

impl NotesImportConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `[import]`: the import's own settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportConfig {
    #[serde(default, skip_serializing_if = "OpenclawSyncConfig::is_default")]
    pub openclaw: OpenclawSyncConfig,
    #[serde(default, skip_serializing_if = "NotesImportConfig::is_default")]
    pub notes: NotesImportConfig,
}

impl OpenclawSyncConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl ImportConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let o = &self.openclaw;
        if let Some(d) = &o.state_dir {
            if d.trim().is_empty() || !d.starts_with('/') && !d.starts_with('~') {
                bail!("[import.openclaw] state_dir is an absolute path (or ~/…), not {d:?}");
            }
        }
        if o.owner_ids.iter().any(|i| i.trim().is_empty()) {
            bail!("[import.openclaw] owner_ids holds an empty id");
        }
        if o.agents
            .iter()
            .any(|a| a.trim().is_empty() || a.contains('/'))
        {
            bail!("[import.openclaw] agents names an agent by its directory's name");
        }
        if !(1..=MAX_SYNC_DEADLINE_MINS).contains(&o.sync_deadline_mins) {
            bail!(
                "[import.openclaw] sync_deadline_mins is 1 to {MAX_SYNC_DEADLINE_MINS} minutes, \
                 not {}: it must stay under the sync unit's own 2 h start timeout or it can \
                 never fire",
                o.sync_deadline_mins
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(mins: u64) -> ImportConfig {
        ImportConfig {
            openclaw: OpenclawSyncConfig {
                sync_deadline_mins: mins,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// The loader's range on the new bound (`b4-import-sync-unbounded-walltime`).
    /// A cap at or over the sync unit's own 2 h start timeout could never fire
    /// -- the client would be killed first and the daemon would carry on -- so
    /// the loader refuses it rather than leaving the run uncapped in effect.
    #[test]
    fn a_sync_deadline_outside_its_range_is_refused() {
        for bad in [0, MAX_SYNC_DEADLINE_MINS + 1, 120, 1440] {
            let e = with(bad)
                .validate()
                .expect_err("a bound outside the range is refused")
                .to_string();
            assert!(
                e.contains("sync_deadline_mins"),
                "the refusal names the key: {e}"
            );
            assert!(e.contains(&bad.to_string()), "and the value given: {e}");
        }
        for ok in [1, 7, 90, MAX_SYNC_DEADLINE_MINS] {
            with(ok)
                .validate()
                .unwrap_or_else(|e| panic!("{ok} minutes is inside the range: {e}"));
        }
    }

    /// The key is read from a config note, under `deny_unknown_fields`, and
    /// nothing else about `[import.openclaw]` moved: before this change the
    /// loader refused the key outright as an unknown field.
    #[test]
    fn the_note_names_the_bound_beside_the_rest_of_the_table() {
        let note = "[openclaw]\n\
                    state_dir = \"/srv/openclaw-store\"\n\
                    agents = [\"wren\"]\n\
                    sync_deadline_mins = 7\n";
        let cfg: ImportConfig = toml::from_str(note).expect("the table parses");
        assert_eq!(cfg.openclaw.sync_deadline_mins, 7);
        assert_eq!(cfg.openclaw.agents, vec!["wren".to_string()]);
        cfg.validate().unwrap();
        // A note that says nothing about it keeps the measured default.
        let plain: ImportConfig = toml::from_str("[openclaw]\nagents = []\n").unwrap();
        assert_eq!(plain.openclaw.sync_deadline_mins, 90);
    }

    /// A config that says nothing gets the measured 90 minutes, and the key is
    /// a TIME key and not a spend one on purpose: the keel's `spend-key` diff
    /// rule reads `usd`, `spend`, `per_day` and `daily` in a config key's name.
    #[test]
    fn the_default_sync_deadline_is_ninety_minutes() {
        assert_eq!(OpenclawSyncConfig::default().sync_deadline_mins, 90);
        assert_eq!(default_sync_deadline_mins(), 90);
        assert!(default_sync_deadline_mins() < MAX_SYNC_DEADLINE_MINS);
        // And an untouched table is still the default one, so a config that
        // names no OpenClaw settings serializes none (`is_default`).
        assert!(OpenclawSyncConfig::default().is_default());
        assert!(ImportConfig::default().is_default());
    }

    /// `[import.notes]`'s two switches read from a config note, and both
    /// default OFF (`b4-theseus-holds-its-own-notes`): the import widens
    /// what Theseus holds to our own operating notes, and the bulk pass
    /// waits on the owner's sight of the rule.
    #[test]
    fn both_agent_notes_switches_default_off() {
        let d = NotesImportConfig::default();
        assert!(!d.enabled, "the path is off");
        assert!(!d.keep_run_state, "and run state is out even when it is on");
        assert!(d.is_default());
        assert!(ImportConfig::default().notes.is_default());
        // A config that names no table gets them off, and one that names the
        // table takes exactly what it says.
        let plain: ImportConfig = toml::from_str("[openclaw]\nagents = []\n").unwrap();
        assert_eq!(plain.notes, NotesImportConfig::default());
        let on: ImportConfig =
            toml::from_str("[notes]\nenabled = true\nkeep_run_state = true\n").unwrap();
        assert!(on.notes.enabled && on.notes.keep_run_state);
        on.validate().unwrap();
        // Under `deny_unknown_fields`, a misspelt switch is refused rather
        // than read as off, which would look exactly like the default.
        assert!(toml::from_str::<ImportConfig>("[notes]\nenable = true\n").is_err());
    }
}
