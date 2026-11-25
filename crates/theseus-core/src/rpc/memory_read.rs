//! `history.read` (issue 46): the MCP server's `context_history`, the history tool's own read with no turn
//! (`history_tool::mcp`), and with no model call. It is the MCP server's one way to look back: the older
//! `memory.lookup` (its `memory_search` and `memory_get`) is gone, and is no method now.
//!
//! - **The MCP connection only when `[mcp_server] memory` is on.** A call with memory off answers `OFF`
//!   ([`Core::mcp_memory_only`]).
//! - **And `history.read` for the owner's terminal** (issue 44): `theseus
//!   history` with a search reads the same page an MCP client does, the
//!   vault's values substituted alike, whatever `[mcp_server] memory` says
//!   (that switch is for other agents). A web or chat surface is refused.
//! - **`thing` is refused in words** (issue 44: it became `label`), before anything is read: the params
//!   take no unknown field back, so an old caller's `thing` would be dropped and another search answered.
//! - **Ledgered**: each call that reads writes one `memory.read` row: the
//!   principal (`mcp`, or `cli` for the terminal), the tool, the client, a
//!   digest of the words (or the id, or the session; never the text), and
//!   how many hits answered. Nothing else is written, and no turn is on the
//!   path.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use theseus_protocol::memory::{HistoryReadParams, HistoryReadResult};
use theseus_protocol::{error_code, method};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;
use crate::fact;

/// What a call with memory off answers.
pub const OFF: &str = "memory is off for MCP clients (`[mcp_server] memory`)";

/// What `history.read` answers a call that still names `thing`.
pub const THING: &str =
    "history.read's thing is now label: call again with label (a topic or a thing by its id, or personal, \
     secret, confidential, injection)";

/// A result as the wire's value.
fn reply<R: serde::Serialize>(r: R) -> Result<serde_json::Value, RpcFailure> {
    serde_json::to_value(r).map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
}

impl Core {
    /// The memory methods that wait: `memory.search` and `history.read`.
    pub(super) async fn mem(
        self: &Arc<Self>,
        name: &str,
        params: serde_json::Value,
        conn: super::server::Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::HISTORY_READ => {
                // Every error of the endpoint, the fixed refusals and a decode or serialization error included,
                // carries the vault's values substituted, whatever it says (review of D6b, §2).
                let scrub = |mut e: RpcFailure| {
                    e.message = crate::history_tool::mcp::said(self, &e.message);
                    e
                };
                // An old caller's `thing` is refused here, before anything is read: the wire's struct would drop
                // it and answer another search (issue 44).
                if params.get("thing").is_some() {
                    return Err(scrub(RpcFailure::new(error_code::INVALID_PARAMS, THING)));
                }
                let p = super::server::parse(params).map_err(scrub)?;
                let r = self
                    .rpc_history_read(p, conn.surface)
                    .await
                    .map_err(scrub)?;
                reply(r).map_err(scrub)
            }
            _ => reply(self.memory_search(super::server::parse(params)?).await?),
        }
    }

    /// `history.read` from a connection of `surface`: the history tool's read for an MCP client, or for the
    /// owner's terminal ([`Core::history_reader`]). Its row's
    /// digest is of the arguments asked (the client's name aside), as one JSON line; its count, the lines
    /// answered (an open is one). A call refused before it reads (a non-MCP surface, memory off, or, in `mem`,
    /// params that do not decode) writes no row.
    pub(super) async fn rpc_history_read(
        self: &Arc<Self>,
        p: HistoryReadParams,
        surface: Surface,
    ) -> Result<HistoryReadResult, RpcFailure> {
        let principal = self.history_reader(surface)?;
        let read = crate::history_tool::mcp::history(self, &p).await;
        let hits = read.as_ref().map_or(0, |r| match r.open {
            Some(_) => 1,
            None => r.lines.len() as u64,
        });
        let asked = HistoryReadParams {
            client: None,
            ..p.clone()
        };
        let asked = serde_json::to_string(&asked).unwrap_or_default();
        self.read_row(
            principal,
            p.client.clone(),
            TOOL_HISTORY,
            &asked,
            hits,
            read.is_ok(),
        )
        .await;
        read.map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))
    }

    /// The two checks every MCP memory read makes first, for `method`: the MCP connection only, and only
    /// while `[mcp_server] memory` is on.
    fn mcp_memory_only(&self, method: &str, surface: Surface) -> Result<(), RpcFailure> {
        if surface != Surface::Mcp {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{method} is the MCP server's: it reads the owner's memory for another agent"
                ),
            ));
        }
        if !self.cfg.mcp_server.memory {
            return Err(RpcFailure::new(error_code::REFUSED, OFF));
        }
        Ok(())
    }

    /// Who may read `history.read` on `surface`, as its row's principal: the owner's terminal (`cli`), whatever
    /// `[mcp_server] memory` says; the MCP connection as every MCP memory read ([`Core::mcp_memory_only`]); a web
    /// or chat surface is refused.
    fn history_reader(&self, surface: Surface) -> Result<&'static str, RpcFailure> {
        match surface {
            Surface::Cli => Ok(CLI),
            Surface::Mcp => self
                .mcp_memory_only(method::HISTORY_READ, surface)
                .map(|()| crate::mcp_server::PRINCIPAL),
            _ => Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{} is the terminal's and the MCP server's: it reads the owner's memory, \
                     and this surface may not",
                    method::HISTORY_READ
                ),
            )),
        }
    }

    /// A read's `memory.read` row: its principal, the client, the tool, the first 16 hex digits of the
    /// SHA-256 of what was asked (never the words), how many answered, and whether it did.
    async fn read_row(
        self: &Arc<Self>,
        principal: &'static str,
        client: Option<String>,
        tool: &'static str,
        asked: &str,
        hits: u64,
        ok: bool,
    ) {
        let digest: String = Sha256::digest(asked.as_bytes())
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect();
        let core = self.clone();
        let _ = tokio::task::spawn_blocking(move || {
            core.rec(None).record(&fact::memory::MemoryRead {
                principal,
                client: client.as_deref(),
                tool,
                digest: &digest,
                hits,
                ok,
            });
        })
        .await;
    }
}

/// The principal of a terminal's `history.read`, as its `memory.read` row names it.
pub const CLI: &str = "cli";

/// The MCP tool `history.read` answers, as its `memory.read` row names it.
pub const TOOL_HISTORY: &str = "context_history";
