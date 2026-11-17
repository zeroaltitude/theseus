//! Servers attached by asking (issue 22, `config_change::mcp_attach`): the
//! owner named a server in chat, saw its card, and said yes, so the daemon
//! wrote `[mcp.servers.<name>]` and loaded it here with no restart.
//!
//! - **Loaded live** beside the configured servers (`ext.rs`'s list), under
//!   the server's own name, so its tools are the same ones a start from the
//!   file offers. Only the tools its trial listed, and the card showed, are
//!   offered until the next start, which offers what a configured server
//!   offers, everything it lists.
//! - **Who and when** are one META record, `mcp.attached`, written in the
//!   frame of the owner's answer; `theseus mcp` and health show it.
//! - **Detach** unloads a server that was loaded this way. A server the
//!   board started from the file stops at the next restart, and the card
//!   says so beforehand.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError};

use serde::{Deserialize, Serialize};

use super::{McpBoard, Server, StoredList};
use crate::config::McpServerConfig;

/// The META key of the record of who attached which server.
pub const META_KEY: &str = "mcp.attached";

/// The most trials' lists kept at once.
const MAX_KEPT: usize = 16;

/// Who attached a server, and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attached {
    /// Who answered the card with a yes.
    pub by: String,
    /// What they answered through.
    pub via: String,
    pub at_ms: u64,
    /// The conversation the server was asked for in.
    pub session_id: String,
}

/// Every attached server, by name.
pub type AttachedSet = BTreeMap<String, Attached>;

impl McpBoard {
    /// Load the server `name` of `cfg` live, offering `stored` at once and
    /// only the tools in `acked` (all it lists, when none), and start it if
    /// the board has started. One of that name already loaded is replaced.
    pub fn load_attached(
        self: &Arc<Self>,
        name: &str,
        cfg: McpServerConfig,
        stored: Option<StoredList>,
        acked: Option<Vec<String>>,
    ) {
        let server = Arc::new(Server {
            acked,
            ..Server::new(name, &cfg, stored)
        });
        self.put_loaded(name, server);
    }

    /// Whether a server of this name is on the board, configured or loaded.
    pub fn has_server(&self, name: &str) -> bool {
        self.servers.contains_key(name) || self.is_loaded(name)
    }

    /// Whether the server `name` was started from the file at the daemon's
    /// start, so that only a restart ends it.
    pub fn is_configured(&self, name: &str) -> bool {
        self.servers.contains_key(name)
    }

    /// Whether this daemon has loaded the `[secrets]` entry `name`: one added
    /// to the file since its start is not known to it until it restarts.
    pub fn knows_secret(&self, name: &str) -> bool {
        self.secrets.states().contains_key(name)
    }

    /// The names of the tools the server `name` offers now.
    pub fn offered_tools(&self, name: &str) -> Vec<String> {
        let found = self.servers.get(name).cloned().or(self.loaded_server(name));
        let Some(s) = found else { return Vec::new() };
        let tools = s.live().tools.clone();
        tools
            .into_iter()
            .filter(|t| s.offers(&t.name))
            .map(|t| t.name)
            .collect()
    }

    /// Keep what the trial of the server `name` listed, to offer at once when
    /// the owner says yes. In memory only, and few: a daemon that restarts
    /// before the answer loads the server and lists it again.
    pub fn keep_listed(&self, name: &str, list: StoredList) {
        let mut kept = self.listed.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.len() >= MAX_KEPT {
            kept.clear();
        }
        kept.insert(name.to_string(), list);
    }

    /// What the trial of `name` listed, once.
    pub fn take_listed(&self, name: &str) -> Option<StoredList> {
        self.listed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name)
    }

    /// Who attached which server, replacing what the board knew.
    pub fn note_attached(&self, set: AttachedSet) {
        *self.attached.lock().unwrap_or_else(PoisonError::into_inner) = set;
    }

    /// Health's and `theseus mcp`'s words for who attached `status`'s server.
    pub(super) fn mark_attached(&self, status: &mut theseus_protocol::mcp::McpServerStatus) {
        let set = self.attached.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(a) = set.get(&status.name) {
            status.attached_by = Some(format!("{} through {}", a.by, a.via));
            status.attached_at_ms = Some(a.at_ms);
        }
    }
}
