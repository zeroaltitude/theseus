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
//! - **A server that fails** is told to the conversation that attached it
//!   (issue 54): one note its next turn reads, once per failure, saying how
//!   it refused in the daemon's words (the HTTP status and a standard code,
//!   never the server's own) and what the owner can do. A server from the
//!   file that no conversation attached tells none; health says it.
//! - **A server whose sign-in is lost** (issue 55: a refresh refused for
//!   good) is told the same way, once, with the one command that signs it in
//!   again: `theseus mcp login <name>`.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, Weak};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{McpBoard, Server, StoredList, CRASHES_TO_FAIL};
use crate::config::McpServerConfig;
use crate::node::{Node, Origin};

/// The META key of the record of who attached which server.
pub const META_KEY: &str = "mcp.attached";

/// The author of the note a failed server's conversation reads.
pub const NOTE_AUTHOR: &str = "mcp";

/// The OAuth error codes (RFC 6749 section 5.2, RFC 6750 section 3.1): the
/// only words of a refusal's body a note repeats.
const CODES: [&str; 11] = theseus_mcp::oauth::CODES;

/// How a server refused, in the daemon's words, from the client's error
/// (`theseus_mcp::Error`): the HTTP status and a standard code from its body,
/// or a JSON-RPC error's number. None for any other failure.
fn refusal(why: &str) -> Option<String> {
    if let Some(rest) = why.strip_prefix("HTTP ") {
        let (status, body) = rest.split_once(": ").unwrap_or((rest, ""));
        let status: u16 = status.parse().ok()?;
        let body: Value = serde_json::from_str(body).unwrap_or_default();
        let code = match &body["error"] {
            Value::String(c) => CODES
                .iter()
                .find(|k| **k == c.as_str())
                .map(|k| k.to_string()),
            e => e["code"].as_i64().map(|n| n.to_string()),
        };
        return Some(match code {
            Some(c) => format!("HTTP {status} ({c})"),
            None => format!("HTTP {status}"),
        });
    }
    let rest = why.strip_prefix("the server answered error ")?;
    let n: i64 = rest.split(':').next()?.parse().ok()?;
    Some(format!("JSON-RPC error {n}"))
}

/// The one line a conversation that attached `server` reads when it fails.
pub(crate) fn failed_note(server: &str, why: &str) -> String {
    let refused = refusal(why);
    let how = match &refused {
        Some(r) => format!("it refused the connection with {r}"),
        None => "its connection did not come up".to_string(),
    };
    let sign_in = match refused
        .as_deref()
        .is_some_and(|r| r.starts_with("HTTP 401"))
    {
        true => {
            format!(
                "; if it wants a sign-in in a browser, it is attached again with a sign-in \
                 (`oauth`), and the owner then signs in once with `theseus mcp login {server}`"
            )
        }
        false => String::new(),
    };
    format!(
        "The MCP server `{server}` this conversation attached is attached but not connected: \
         {how}, and after {CRASHES_TO_FAIL} tries Theseus stopped trying. The owner can start it \
         again with `theseus mcp restart {server}`{sign_in}."
    )
}

/// The one line a conversation that attached `server` reads when its
/// sign-in is lost: `why` is the daemon's own words (`mcp::oauth`).
pub(crate) fn sign_in_note(server: &str, why: &str) -> String {
    format!(
        "The MCP server `{server}` this conversation attached is attached but not connected: \
         {why}, so it needs a sign-in again, and Theseus stopped trying. The owner signs in once \
         with `theseus mcp login {server}`, from their own terminal."
    )
}

/// How many seconds a note waits for a running turn of its conversation.
const NOTE_TRIES: u32 = 3_600;

/// Write `node` into `session`, in a frame of its execution: false while a
/// turn holds it, so the caller tries again.
fn write_note(core: &crate::rpc::Core, session: &str, node: &Node) -> anyhow::Result<bool> {
    let exec = core
        .store
        .get_session::<crate::session::SessionRecord>(session)?
        .and_then(|r| r.execution_id);
    let Some(exec) = exec else {
        core.store.append(&[node.record()?])?;
        return Ok(true);
    };
    let mut wrote = false;
    core.kernel.frame(&[&exec], |k| {
        if k.holds_turn(&exec) {
            return Ok(());
        }
        k.stage(&[node.record()?])?;
        wrote = true;
        Ok(())
    })?;
    Ok(wrote)
}

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
        // A sign-in loaded for an earlier server of that name is read again.
        self.signed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
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

    /// Tell the conversation that attached `server` that it failed, with
    /// `why` in the daemon's words ([`failed_note`]): a note in that session,
    /// as a keep card's end is told (`vault_keep::told`), written off the
    /// runtime's workers. A server no conversation attached tells none.
    pub(super) fn tell_failed(&self, server: &str, why: &str) {
        if self.signs_in(server) {
            return self.tell(server, failed_note(server, why));
        }
        // A server with no sign-in: stored and delivered exactly as on main.
        let session = {
            let set = self.attached.lock().unwrap_or_else(PoisonError::into_inner);
            set.get(server).map(|a| a.session_id.clone())
        };
        let Some(session) = session.filter(|s| !s.is_empty()) else {
            return;
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return;
        };
        let text = failed_note(server, why);
        let node = Node::relayed(&session, None, Origin::Harness, NOTE_AUTHOR, &text);
        let write = move || {
            if let Err(e) = node.record().and_then(|r| core.store.append(&[r])) {
                tracing::warn!(error = %format!("{e:#}"), "the note of a failed MCP server was not written");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    /// Whether the configured or loaded server `server` signs in (`oauth`):
    /// its notes wait for a running turn of their conversation (`tell`).
    fn signs_in(&self, server: &str) -> bool {
        self.servers
            .get(server)
            .cloned()
            .or_else(|| self.loaded_server(server))
            .is_some_and(|s| s.cfg.oauth.is_some())
    }

    /// Tell the conversation that attached `server` that its sign-in is lost
    /// ([`sign_in_note`]), as a failure is told.
    pub(super) fn tell_sign_in(&self, server: &str, why: &str) {
        self.tell(server, sign_in_note(server, why));
    }

    fn tell(&self, server: &str, text: String) {
        let session = {
            let set = self.attached.lock().unwrap_or_else(PoisonError::into_inner);
            set.get(server).map(|a| a.session_id.clone())
        };
        let Some(session) = session.filter(|s| !s.is_empty()) else {
            return;
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return;
        };
        let node = Node::relayed(&session, None, Origin::Harness, NOTE_AUTHOR, &text);
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            let _ = write_note(&core, &session, &node);
            return;
        };
        // A turn of that conversation may be running (a sign-in is most often
        // lost in a call of its own): the note waits for its end, as a publish
        // does, so the turn's own transcript stays the store's.
        rt.spawn(async move {
            for _ in 0..NOTE_TRIES {
                let (c, s, n) = (core.clone(), session.clone(), node.clone());
                match tokio::task::spawn_blocking(move || write_note(&c, &s, &n)).await {
                    Ok(Ok(false)) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
                    Ok(Ok(true)) => return,
                    Ok(Err(e)) => {
                        tracing::warn!(error = %format!("{e:#}"), "the note of an MCP server was not written");
                        return;
                    }
                    Err(_) => return,
                }
            }
            tracing::warn!("the note of an MCP server was not written: its conversation's turn ran on");
        });
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
