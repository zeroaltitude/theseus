//! `mcp.restart` (M7 36b): one MCP server started again, a failed one
//! included. `mcp.list` is the board's own (`McpBoard::list`).

use theseus_protocol::error_code;
use theseus_protocol::mcp::{McpRestartParams, McpRestartResult};

use super::server::RpcFailure;
use super::Core;

impl Core {
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
