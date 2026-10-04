//! `mcp.restart` (M7 36b): one MCP server started again, a failed one
//! included. `mcp.list` is the board's own (`McpBoard::list`).
//! `mcp.prompt.list` and a `turn.submit`'s prompt (36c) are here too.

use serde_json::Value;
use theseus_protocol::mcp::{
    McpPromptListParams, McpPromptListResult, McpPromptRef, McpRestartParams, McpRestartResult,
};
use theseus_protocol::{error_code, PlaceClass};

use crate::mcp::prompts::{PromptInput, Refusal};

use super::server::RpcFailure;
use super::Core;

impl Core {
    /// `mcp.prompt.list`: the prompts of one server, or of every server.
    pub(super) fn mcp_prompt_list(&self, params: Value) -> Result<McpPromptListResult, RpcFailure> {
        // No params lists every server's prompts.
        let params = if params.is_null() {
            serde_json::json!({})
        } else {
            params
        };
        let p: McpPromptListParams = super::server::parse(params)?;
        if let Some(name) = &p.server {
            if self.mcp.server(name).is_none() {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!("no MCP server {name:?} is configured"),
                ));
            }
        }
        Ok(McpPromptListResult {
            prompts: self.mcp.prompt_infos(p.server.as_deref()),
        })
    }

    /// A turn's prompt, asked of its server before the turn opens or writes
    /// anything. Only a session in a private place runs one: MCP is not
    /// public, and a server's words would reach a shared place's model.
    pub(super) async fn resolve_prompt(
        &self,
        r: &McpPromptRef,
        session_id: Option<&str>,
    ) -> Result<PromptInput, RpcFailure> {
        if let Some(sid) = session_id {
            if self.runner.class_of(sid) == crate::places::PlaceClass::Shared {
                return Err(RpcFailure::new(
                    error_code::REFUSED,
                    format!(
                        "an MCP prompt runs only in a private place (the {} place this session \
                         speaks in is shared), since a server's words would reach a shared \
                         place's model; run it from the CLI, the cockpit, or a DM with an owner",
                        PlaceClass::Shared.as_str()
                    ),
                ));
            }
        }
        self.mcp.resolve_prompt(r).await.map_err(|e| match e {
            Refusal::Invalid(m) => RpcFailure::new(error_code::INVALID_PARAMS, m),
            Refusal::Unavailable(m) => RpcFailure::new(error_code::INTERNAL, m),
        })
    }

    pub(super) fn mcp_restart(&self, p: McpRestartParams) -> Result<McpRestartResult, RpcFailure> {
        match self.mcp.restart(&p.name) {
            Some(crate::mcp::State::Disabled) => Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                format!(
                    "MCP server {} is disabled (enabled = false in its [mcp.servers] table)",
                    p.name
                ),
            )),
            Some(was) => Ok(McpRestartResult {
                name: p.name,
                was: was.as_str().into(),
            }),
            None => {
                let known: Vec<String> = self.mcp.status().into_iter().map(|s| s.name).collect();
                Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!(
                        "no MCP server {:?} is configured (the servers: {})",
                        p.name,
                        if known.is_empty() {
                            "none".into()
                        } else {
                            known.join(", ")
                        }
                    ),
                ))
            }
        }
    }
}
