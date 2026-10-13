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

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenclawSyncConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
}

/// `[import]`: the import's own settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportConfig {
    #[serde(default, skip_serializing_if = "OpenclawSyncConfig::is_default")]
    pub openclaw: OpenclawSyncConfig,
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
        Ok(())
    }
}
