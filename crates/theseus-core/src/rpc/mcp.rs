//! The MCP server's connection (step 41b, `crate::mcp_server`): what a
//! request on `Surface::Mcp` may do, the session it opens, and its rows.

use serde_json::Value;
use theseus_protocol::{error_code, LedgerKind, SessionOpenParams};

use super::server::RpcFailure;
use super::Core;
use crate::ledger::LedgerRow;
use crate::mcp_server;
use crate::narrative::narrate;

impl Core {
    /// A request on `Surface::Mcp`, before its method runs: one the server's
    /// tools need, or an owner's act, which its judgment refuses; and a turn
    /// only into a session an MCP client opened.
    pub(super) fn mcp_admit(&self, method: &str, params: &Value) -> Result<(), RpcFailure> {
        if !mcp_server::allowed(method) {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!("the MCP server's connection may not call {method}"),
            ));
        }
        if method != theseus_protocol::method::TURN_SUBMIT {
            return Ok(());
        }
        let Some(id) = params.get("session_id").and_then(Value::as_str) else {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                "a turn from the MCP server names a session it opened",
            ));
        };
        let exec = self.session_execution(id)?;
        match exec.is_some_and(|e| mcp_server::own(&e.authority)) {
            true => Ok(()),
            false => Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{id} is not a conversation an MCP client opened: open one with \
                     conversation_open"
                ),
            )),
        }
    }

    /// The execution of session `id`, if it has one: not found is the
    /// client's error.
    fn session_execution(&self, id: &str) -> Result<Option<theseus_kernel::Execution>, RpcFailure> {
        let rec = self.session(id)?;
        let Some(exec_id) = rec.execution_id else {
            return Ok(None);
        };
        self.kernel
            .execution(&exec_id)
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
    }

    /// `session.open` on `Surface::Mcp`: principal `mcp`, `[mcp_server]`'s
    /// floor as a ceiling, its spend limit, and the label the server gives
    /// (`mcp <client>`). Narrated as the client's.
    pub(super) fn mcp_open(
        &self,
        p: SessionOpenParams,
    ) -> Result<theseus_protocol::SessionInfo, RpcFailure> {
        let m = &self.cfg.mcp_server;
        let limit = theseus_kernel::usd_to_micros(m.spend_limit_usd);
        let label = p.label.clone().unwrap_or_else(|| "mcp".into());
        let rec = self.open_session_as(p, mcp_server::authority(m), Some(limit))?;
        if self.narrator.on() {
            narrate!(
                self.narrator,
                Session,
                Some(&rec.session_id),
                None,
                "MCP client {} opened session {}; its calls that act wait for the operator.",
                label.strip_prefix("mcp ").unwrap_or(&label),
                crate::narrative::short(&rec.session_id)
            );
        }
        let mut info = rec.info();
        info.execution_state = Some("waiting".into());
        Ok(info)
    }

    /// One answered MCP tool call (`mcp_server.call`): the tool, the
    /// session, the client's name, the latency, and whether it answered.
    /// A frame each; the caller writes it off the runtime's workers.
    pub fn mcp_called(&self, session_id: Option<&str>, data: Value) {
        let row = LedgerRow::new(LedgerKind::McpServerCall, session_id, None, data);
        if let Err(e) = self.store.append_ledger(&row) {
            tracing::warn!(error = %e, "mcp server: its call's row was not written");
        }
    }

    /// A refused request (`mcp_server.refused`): the server reports each
    /// kind at most once a minute, with the number it stands for.
    pub fn mcp_refused(&self, data: Value) {
        let row = LedgerRow::new(LedgerKind::McpServerRefused, None, None, data);
        if let Err(e) = self.store.append_ledger(&row) {
            tracing::warn!(error = %e, "mcp server: its refusal's row was not written");
        }
    }
}
