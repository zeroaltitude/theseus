//! An acked extension loaded (M7 43b, design §2.7): the `extensions` META
//! record, what the board runs from it, and its start after serving.
//!
//! - **The record** is one META key, `extensions`: each loaded name to what
//!   the ack loaded (digest, command, tools, capabilities, who acked and
//!   when, the proposing session), beside 36b's `mcp.tools.<server>`. A key
//!   of its own, so no store format bump. A start reads it once.
//! - **The ack's frame** writes it, with `extend.loaded`, the server's stored
//!   list (the tools its trial listed, so a new version never offers an old
//!   one's), and the replaced version's manifest. After that frame the board
//!   loads `ext-<name>`, and its tools are offered from the next turn.
//! - **What runs** is the frozen copy, in L1, with the network the proposal
//!   asked and the ack granted (none unless asked), and no secret. Its
//!   results are outside text unless its network is off (Q22).
//! - **Never wider** (§3.9): the proposing session's place and its ceiling,
//!   as they were at the ack, are recorded with the load, and that ceiling's
//!   floor holds every call of its tools, wherever it is called (`floor`).
//!   Where its tools are offered is where MCP tools are: private places,
//!   within each place's own ceiling (`mcp:ext-<name>`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use theseus_protocol::PlaceCeiling;
use theseus_store::{kinds, NewRecord};

use super::{Capabilities, Manifest, ProposedBy, SERVER_PREFIX};
use crate::ceiling::Ceiling;
use crate::config::McpServerConfig;
use crate::mcp::StoredList;
use crate::policy::Decision;

/// The META key of the loaded extensions.
pub const RECORD: &str = "extensions";

/// One loaded extension, as the ack loaded it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loaded {
    pub name: String,
    pub digest: String,
    pub description: String,
    pub command: Vec<String>,
    /// The frozen copy that runs.
    pub frozen: String,
    /// The workspace directory it was frozen from: never what runs.
    pub source: String,
    /// The tools its trial listed.
    pub tools: Vec<String>,
    pub capabilities: Capabilities,
    pub acked_by: String,
    pub acked_via: String,
    pub acked_at_ms: u64,
    /// The question the ack answered.
    pub question: String,
    pub proposed_by: ProposedBy,
    /// The proposing session's place at the ack, as health names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_name: Option<String>,
    /// That place's ceiling at the ack: its floor holds every call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<PlaceCeiling>,
    /// The digest of the version it replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced: Option<String>,
}

impl Loaded {
    /// Its server on the board.
    pub fn server(&self) -> String {
        server_name(&self.name)
    }

    /// Its tools' canonical names.
    pub fn canonical(&self) -> Vec<String> {
        let server = self.server();
        self.tools
            .iter()
            .map(|t| format!("{}{server}/{t}", crate::policy::MCP_PREFIX))
            .collect()
    }

    /// What the board runs: the frozen copy, never the workspace, in L1,
    /// with the network it was acked and no secret.
    pub fn server_cfg(&self) -> McpServerConfig {
        let mut cfg = super::trial_cfg(
            &self.command,
            &PathBuf::from(&self.frozen),
            &self.capabilities.network,
        );
        cfg.external = !self.capabilities.network.is_empty();
        // A configured server's default; the trial's is shorter.
        cfg.call_timeout_secs = 110;
        cfg
    }

    /// The ceiling it was loaded under, as the gate reads it.
    fn ceiling(&self) -> Option<Ceiling> {
        let place = self.place_name.as_deref().unwrap_or("the proposing place");
        Ceiling::new(place, self.ceiling.as_ref()?)
    }
}

pub fn server_name(name: &str) -> String {
    format!("{SERVER_PREFIX}{name}")
}

/// The META record `extensions`: every loaded extension, by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadedSet {
    pub loaded: BTreeMap<String, Loaded>,
}

impl LoadedSet {
    pub fn read(store: &crate::store::Store) -> anyhow::Result<Self> {
        Ok(store.get_meta::<Self>(RECORD)?.unwrap_or_default())
    }

    pub fn record(&self) -> anyhow::Result<NewRecord> {
        NewRecord::json(kinds::META, Some(RECORD), self)
    }
}

/// A manifest's tools as the board's stored list for its server.
pub fn stored_list(m: &Manifest) -> StoredList {
    let tools: Vec<theseus_mcp::types::Tool> = m
        .tools
        .iter()
        .filter_map(|t| {
            serde_json::from_value(serde_json::json!({
                "name": t.name, "description": t.description, "inputSchema": t.input_schema,
            }))
            .ok()
        })
        .collect();
    StoredList {
        digest: crate::mcp::digest(&tools),
        tools,
    }
}

/// The stored list's record for `server`.
pub fn stored_record(server: &str, list: &StoredList) -> anyhow::Result<NewRecord> {
    NewRecord::json(
        kinds::META,
        Some(&format!("{}{server}", crate::mcp::STORED_PREFIX)),
        list,
    )
}

/// The loaded extensions' ceilings, by server, for the gate.
#[derive(Default)]
pub struct Floors {
    by_server: RwLock<BTreeMap<String, Ceiling>>,
}

impl Floors {
    pub fn set(&self, l: &Loaded) {
        let mut m = self
            .by_server
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        match l.ceiling() {
            Some(c) => m.insert(l.server(), c),
            None => m.remove(&l.server()),
        };
    }

    pub fn clear(&self, server: &str) {
        self.by_server
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(server);
    }

    /// `d` for a call of `tool` at no looser a posture than the floor of the
    /// ceiling its extension was loaded under.
    pub fn floor(&self, tool: &str, d: Decision, summary: &str) -> Decision {
        let Some(server) = tool
            .strip_prefix(crate::policy::MCP_PREFIX)
            .and_then(|r| r.split('/').next())
            .filter(|s| s.starts_with(SERVER_PREFIX))
        else {
            return d;
        };
        let m = self
            .by_server
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        match m.get(server) {
            Some(c) => c.floor(d, tool, summary),
            None => d,
        }
    }
}

impl crate::rpc::Core {
    /// At build, before serving: each loaded extension on the board, its
    /// stored list offered at once (one META key each, as a configured
    /// server's), started with the rest after serving.
    pub(crate) fn seed_extensions(&self) -> anyhow::Result<()> {
        for l in LoadedSet::read(&self.store)?.loaded.values() {
            self.board_load(l, crate::mcp::read_stored(&self.store, &l.server()));
        }
        Ok(())
    }

    /// `l` on the board, offering `stored`, and its floor at the gate.
    pub(crate) fn board_load(&self, l: &Loaded, stored: Option<StoredList>) {
        self.tools.extend.floors.set(l);
        self.mcp.load_extension(&l.server(), l.server_cfg(), stored);
    }
}

/// What an ack loads, built before its frame.
pub(crate) struct Load {
    pub set: LoadedSet,
    pub loaded: Loaded,
    pub stored: StoredList,
    /// The version it replaces, its manifest now `replaced`.
    pub replaced: Option<Manifest>,
}

impl Load {
    /// Its records, for the ack's frame: the `extensions` record, the
    /// server's stored list, and the replaced version's manifest.
    pub fn records(&self) -> anyhow::Result<Vec<NewRecord>> {
        let mut out = vec![
            self.set.record()?,
            stored_record(&self.loaded.server(), &self.stored)?,
        ];
        if let Some(m) = &self.replaced {
            out.push(m.record()?);
        }
        Ok(out)
    }

    pub fn fact(&self) -> crate::fact::extend::ExtendLoaded<'_> {
        let l = &self.loaded;
        crate::fact::extend::ExtendLoaded {
            name: &l.name,
            digest: &l.digest,
            tools: &l.tools,
            network: &l.capabilities.network,
            replaced: l.replaced.as_deref(),
            by: &l.acked_by,
            place: l.place_name.as_deref(),
            ceiling: l.ceiling.as_ref(),
        }
    }
}

impl crate::rpc::Core {
    /// The load an ack of `m`, by `by` through `via` at `at_ms`, makes: the
    /// record with it in, replacing a loaded older version, and the
    /// proposing session's place and ceiling now. Under `writes`.
    pub(crate) fn load_of(
        &self,
        m: &Manifest,
        question: &str,
        by: &str,
        via: &str,
        at_ms: u64,
    ) -> anyhow::Result<Load> {
        let mut set = LoadedSet::read(&self.store)?;
        let view = self.runner.view_of(&m.proposed_by.session_id);
        let old = set.loaded.get(&m.name).filter(|o| o.digest != m.digest);
        let replaced = match old {
            Some(o) => self
                .store
                .get_meta::<Manifest>(&super::manifest_key(&o.name, &o.digest))?
                .map(|mut om| {
                    om.state = "replaced".into();
                    om.note = Some(format!("replaced by {}", m.digest));
                    om
                }),
            None => None,
        };
        let loaded = Loaded {
            name: m.name.clone(),
            digest: m.digest.clone(),
            description: m.description.clone(),
            command: m.command.clone(),
            frozen: m.frozen.clone(),
            source: m.source.clone(),
            tools: m.tools.iter().map(|t| t.name.clone()).collect(),
            capabilities: m.capabilities.clone(),
            acked_by: by.into(),
            acked_via: via.into(),
            acked_at_ms: at_ms,
            question: question.into(),
            proposed_by: m.proposed_by.clone(),
            place_name: view
                .ceiling
                .map(|c| c.place.clone())
                .or_else(|| m.proposed_by.place.clone()),
            ceiling: view.ceiling.map(|c| c.wire.clone()),
            replaced: old.map(|o| o.digest.clone()),
        };
        set.loaded.insert(m.name.clone(), loaded.clone());
        Ok(Load {
            set,
            loaded,
            stored: stored_list(m),
            replaced,
        })
    }
}
