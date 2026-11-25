//! `mcp.login.begin`, `mcp.login.finish` and `mcp.logout` (issue 55): the
//! owner's, from the CLI on this machine. Refused from Discord, from the web
//! UI and from an MCP client, as `google.auth.*` are; the CLI refuses them
//! inside a Theseus job (`OPERATORS`), a speed bump, L1 the boundary. The
//! paste travels here in a param, never an argument; no answer holds a
//! token, a code or a secret.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::mcp::{McpLoginBeginParams, McpLoginFinishParams, McpLogoutParams};
use theseus_protocol::{error_code, method};

use super::server::{parse, reply, Conn, RpcFailure};
use crate::approval::Surface;

impl crate::Core {
    /// The three methods, from the dispatch's one arm.
    pub(super) async fn rpc_mcp_login(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
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
