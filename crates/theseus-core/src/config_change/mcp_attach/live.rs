//! The yes of an attach or a detach, on the board (issue 22): who attached
//! which server is one META record in the answer's own frame, and once the
//! frame is written the server is loaded or unloaded here, with no restart.
//!
//! The server loaded is the table as the file now holds it, parsed as a start
//! parses it, never the proposal: what runs is what was written.

use anyhow::Result;
use serde_json::Value;
use theseus_store::{kinds, NewRecord};

use crate::mcp::attached::{Attached, AttachedSet, META_KEY};
use crate::rpc::Core;

/// What the yes does to the board, prepared before the answer's frame.
pub(crate) struct Live {
    set: AttachedSet,
    op: String,
    name: String,
    listed: bool,
    tools: Vec<String>,
    /// The trial was refused for want of a sign-in (issue 59).
    signs_in: bool,
}

impl Live {
    /// The record of who attached which, for the answer's frame.
    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(META_KEY), &self.set)
    }
}

impl Core {
    /// What a yes to a question that carries an `mcp` marker does to the
    /// board; none for any other question.
    pub(crate) fn mcp_prepare(
        &self,
        args: &Value,
        by: &str,
        via: &str,
        session_id: &str,
    ) -> Result<Option<Live>> {
        let Some(m) = args.get("mcp") else {
            return Ok(None);
        };
        let name = m["name"].as_str().unwrap_or_default().to_string();
        let op = m["op"].as_str().unwrap_or_default().to_string();
        let mut set: AttachedSet = self.store.get_meta(META_KEY)?.unwrap_or_default();
        match op.as_str() {
            "attach" => {
                set.insert(
                    name.clone(),
                    Attached {
                        by: by.to_string(),
                        via: via.to_string(),
                        at_ms: theseus_protocol::now_unix_ms(),
                        session_id: session_id.to_string(),
                    },
                );
            }
            _ => {
                set.remove(&name);
            }
        }
        Ok(Some(Live {
            set,
            op,
            name,
            listed: m["listed"].as_bool().unwrap_or(false),
            signs_in: m["signs_in"].as_bool().unwrap_or(false),
            tools: m["tools"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        }))
    }

    /// The board after the answer's frame: the server loaded as the file now
    /// holds it, or unloaded.
    pub(crate) fn mcp_apply(&self, live: Live) {
        self.mcp.note_attached(live.set.clone());
        match live.op.as_str() {
            "attach" => {
                let Some(server) = self.written_server(&live.name) else {
                    tracing::warn!(server = %live.name, "an attached MCP server is not in the file that was written, so it was not loaded");
                    return;
                };
                // The tools the trial listed and the card showed are the ones
                // offered until the next start; a trial that listed none
                // leaves the list to the server.
                let acked = live.listed.then_some(live.tools);
                let stored = self.mcp.take_listed(&live.name);
                let signs_in = live.signs_in && server.oauth.is_some();
                self.mcp.load_attached(&live.name, server, stored, acked);
                // The owner's Sign in card, once for this attach (issue 59).
                if signs_in {
                    let at = live.set.get(&live.name).map_or(0, |a| a.at_ms);
                    self.mcp.offer_sign_in(&live.name, &format!("attach:{at}"));
                }
            }
            _ => {
                self.mcp.unload_extension(&live.name);
            }
        }
    }

    /// `[mcp.servers.<name>]` as the settings file holds it now.
    fn written_server(&self, name: &str) -> Option<crate::config::McpServerConfig> {
        let file = self.tools.config_change.file.as_ref()?;
        let text = std::fs::read_to_string(file).ok()?;
        let (config, _) = crate::Config::parse(&text).ok()?;
        config.mcp.servers.get(name).cloned()
    }

    /// At build: who attached which, so health says it after a restart.
    pub(crate) fn seed_attached(&self) -> Result<()> {
        let set: AttachedSet = self.store.get_meta(META_KEY)?.unwrap_or_default();
        self.mcp.note_attached(set);
        Ok(())
    }
}
