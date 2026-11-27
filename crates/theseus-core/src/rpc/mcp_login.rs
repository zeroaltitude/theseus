//! `mcp.login.begin`, `mcp.login.finish` and `mcp.logout` (issue 55): admitted
//! from the CLI's surface, as `google.auth.*` are, and (issue 59) from the
//! owner's own press of a control in a private place on Discord; refused from
//! the web UI, from an MCP client, and from Discord for anyone else or any
//! other place. On Discord the presser is the one the binding read off the
//! press, judged by `places::owner_in_private` as every press is: an owner, in
//! a direct message or a channel bound private. A turn never presses: it can
//! only offer the control (`mcp.sign_in_offer`), and nothing a turn calls
//! reaches these methods. What stops an agent's job is what stopped it before:
//! the surface; the CLI's own refusal inside a job (`OPERATORS`, which a
//! process can step around by clearing its environment); and the L1 sandbox,
//! which hides this socket from a sandboxed job. A process at L0 that reaches
//! the socket is the CLI's surface, and nothing here asks whose it is. The
//! paste travels here in a param, never an argument; no answer holds a token,
//! a code or a secret.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use theseus_protocol::mcp::{McpLoginBeginParams, McpLoginFinishParams, McpLogoutParams};
use theseus_protocol::{error_code, method, DiscordOrigin};

use super::server::{parse, reply, Conn, RpcFailure};
use crate::approval::Surface;
use crate::mcp::offer::NOT_YOURS;

/// The presser a Discord press names, read before anything else of the params.
#[derive(Deserialize, Default)]
struct Presser {
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    discord: Option<DiscordOrigin>,
}

impl crate::Core {
    /// Whether `name` may be called on `conn` with `params`: the CLI; or, on
    /// Discord, the owner's own press in a private place. `Err` says why not.
    fn mcp_login_admitted(
        &self,
        name: &str,
        params: &Value,
        conn: &Conn<'_>,
    ) -> Result<(), RpcFailure> {
        match conn.surface {
            Surface::Cli => Ok(()),
            Surface::Discord => {
                let p: Presser = serde_json::from_value(params.clone()).unwrap_or_default();
                if p.discord.is_none() {
                    return Err(RpcFailure::new(error_code::REFUSED, NOT_YOURS));
                }
                let who = conn.answerer(p.author, p.discord);
                crate::places::owner_in_private(&who, &self.runner.place_rule, &self.cfg).map_err(
                    |why| RpcFailure::new(error_code::REFUSED, format!("{NOT_YOURS}: {why}")),
                )
            }
            _ => {
                let verb = if name == method::MCP_LOGOUT {
                    "logout"
                } else {
                    "login"
                };
                Err(RpcFailure::new(
                    error_code::REFUSED,
                    format!(
                        "{name} is the operator's, from the CLI on this machine: theseus mcp {verb}"
                    ),
                ))
            }
        }
    }

    /// The three methods, from the dispatch's one arm.
    pub(super) async fn rpc_mcp_login(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        self.mcp_login_admitted(name, &params, &conn)?;
        let failed = |e: String| RpcFailure::new(error_code::INVALID_PARAMS, e);
        match name {
            method::MCP_LOGIN_BEGIN => {
                let p: McpLoginBeginParams = parse(params)?;
                // A Discord press is told the kind alone (issue 62).
                reply(
                    self.mcp
                        .login_begin(&p.name, p.manual, on_wsl())
                        .await
                        .map_err(|f| failed(f.said_to(conn.surface)))?,
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

/// Whether this machine is WSL, for a sign-in's begin: the machine's answer, or a test's.
fn on_wsl() -> bool {
    #[cfg(test)]
    if let Some(wsl) = TEST_WSL.with(std::cell::Cell::get) {
        return wsl;
    }
    crate::google::auth::on_wsl()
}

#[cfg(test)]
thread_local! {
    /// Tests: the answer to whether this machine is WSL, in place of the machine's.
    pub(crate) static TEST_WSL: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}
