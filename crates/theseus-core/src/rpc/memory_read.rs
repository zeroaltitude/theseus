//! `memory.lookup` (theseus-02i1h): what the MCP server's `memory_search` and
//! `memory_get` ask of the owner's private memory, with no model call. One
//! read-only method, served by `memory_lookup`'s own functions
//! (`memory_lookup::mcp`), never a second pipeline.
//!
//! - **Only the MCP connection, only when `[mcp_server] memory` is on.** Any
//!   other surface is refused (a shared place's client could be one), and a
//!   call with memory off answers `OFF`.
//! - **Ledgered**: each call that reads writes one `memory.read` row: the
//!   principal `mcp`, the tool, the client, a digest of the words (or the id,
//!   or the session; never the text), and how many hits answered. Nothing
//!   else is written, and no turn is on the path.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use theseus_protocol::error_code;
use theseus_protocol::memory::{MemoryLookupParams, MemoryLookupResult};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;
use crate::fact;

/// What a call with memory off answers.
pub const OFF: &str = "memory is off for MCP clients (`[mcp_server] memory`)";

/// A result as the wire's value.
fn reply<R: serde::Serialize>(r: R) -> Result<serde_json::Value, RpcFailure> {
    serde_json::to_value(r).map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
}

impl Core {
    /// The memory methods that wait: `memory.search` and `memory.lookup`.
    pub(super) async fn mem(
        self: &Arc<Self>,
        name: &str,
        params: serde_json::Value,
        conn: super::server::Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            theseus_protocol::method::MEMORY_LOOKUP => reply(
                self.rpc_memory_lookup(super::server::parse(params)?, conn.surface)
                    .await?,
            ),
            _ => reply(self.memory_search(super::server::parse(params)?).await?),
        }
    }

    /// `memory.lookup` from a connection of `surface`.
    pub(super) async fn rpc_memory_lookup(
        self: &Arc<Self>,
        p: MemoryLookupParams,
        surface: Surface,
    ) -> Result<MemoryLookupResult, RpcFailure> {
        if surface != Surface::Mcp {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                "memory.lookup is the MCP server's: it reads the owner's memory for another agent",
            ));
        }
        if !self.cfg.mcp_server.memory {
            return Err(RpcFailure::new(error_code::REFUSED, OFF));
        }
        let (tool, asked) = match p.op.as_str() {
            "search" => ("memory_search", p.words.clone()),
            "get" => ("memory_get", p.id.clone().or_else(|| p.session.clone())),
            other => {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!("memory.lookup's op is search or get, not {other:?}"),
                ))
            }
        };
        let read = match tool {
            "memory_search" => crate::memory_lookup::mcp::search(self, &p).await,
            _ => crate::memory_lookup::mcp::get(self, &p).await,
        };
        let hits = read.as_ref().map_or(0, |r| r.hits.len() as u64);
        let digest: String = Sha256::digest(asked.unwrap_or_default().trim().as_bytes())
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect();
        let core = self.clone();
        let (client, ok) = (p.client.clone(), read.is_ok());
        let _ = tokio::task::spawn_blocking(move || {
            core.rec(None).record(&fact::memory::MemoryRead {
                principal: crate::mcp_server::PRINCIPAL,
                client: client.as_deref(),
                tool,
                digest: &digest,
                hits,
                ok,
            });
        })
        .await;
        read.map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))
    }
}
