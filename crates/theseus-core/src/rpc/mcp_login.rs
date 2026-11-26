//! `mcp.login.begin`, `mcp.login.finish` and `mcp.logout` (issue 55): admitted
//! from the CLI's surface alone, as `google.auth.*` are, and refused from
//! Discord, from the web UI and from an MCP client. What stops an agent's job
//! is exactly that: the surface; the CLI's own refusal inside a job
//! (`OPERATORS`, which a process can step around by clearing its
//! environment); and the L1 sandbox, which hides this socket from a
//! sandboxed job. A process at L0 that reaches the socket is the CLI's
//! surface, and nothing here asks whose it is. The paste travels here in a
//! param, never an argument; no answer holds a token, a code or a secret.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::mcp::{McpLoginBeginParams, McpLoginFinishParams, McpLogoutParams};
use theseus_protocol::{error_code, method};

use super::server::{parse, reply, Conn, RpcFailure};
use crate::approval::Surface;

impl crate::Core {
    /// The three methods, from the dispatch's one arm, which `mcp.restart`
    /// shares so the dispatch stays within clippy's complexity (the shape
    /// budget). A restart is admitted as on main: this surface check is the
    /// sign-in's alone.
    pub(super) async fn rpc_mcp_login(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name == method::MCP_RESTART {
            return reply(self.mcp_restart(parse(params)?)?);
        }
        if conn.surface != Surface::Cli {
            let verb = if name == method::MCP_LOGOUT {
                "logout"
            } else {
                "login"
            };
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{name} is the operator's, from the CLI on this machine: theseus mcp {verb}"
                ),
            ));
        }
        let failed = |e: String| RpcFailure::new(error_code::INVALID_PARAMS, e);
        match name {
            method::MCP_LOGIN_BEGIN => {
                let p: McpLoginBeginParams = parse(params)?;
                reply(
                    self.mcp
                        .login_begin(&p.name, p.manual, crate::google::auth::on_wsl())
                        .await
                        .map_err(failed)?,
                )
            }
            method::MCP_LOGIN_FINISH => {
                let p: McpLoginFinishParams = parse(params)?;
                reply(self.mcp.login_finish(&p).await.map_err(failed)?)
            }
            _ => {
                let p: McpLogoutParams = parse(params)?;
                reply(self.mcp.logout(&p.name).await.map_err(failed)?)
            }
        }
    }
}
