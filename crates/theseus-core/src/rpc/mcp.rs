//! The MCP methods (M7). `mcp.restart` (36b): one MCP server started again,
//! a failed one included; `mcp.list` is the board's own (`McpBoard::list`).
//! `mcp.prompt.list` and a `turn.submit`'s prompt (36c) are here too. And
//! the MCP server's connection (step 41b, `crate::mcp_server`): what a
//! request on `Surface::Mcp` may do, the session it opens, and its rows.

use serde_json::Value;
use theseus_protocol::mcp::{
    McpPromptListParams, McpPromptListResult, McpPromptRef, McpRestartParams, McpRestartResult,
};
use theseus_protocol::{error_code, LedgerKind, PlaceClass, SessionOpenParams};

use crate::mcp::prompts::{PromptInput, Refusal};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;
use crate::ledger::LedgerRow;
use crate::mcp_server;
use crate::narrative::narrate;

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

impl Core {
    /// A request on `Surface::Mcp`, before its method runs: one the server's
    /// tools need, or an owner's act, which its judgment refuses; and a turn
    /// only into a session an MCP client opened. Any other surface passes.
    pub(super) fn mcp_admit(
        &self,
        surface: Surface,
        method: &str,
        params: &Value,
    ) -> Result<(), RpcFailure> {
        if surface != Surface::Mcp {
            return Ok(());
        }
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

    /// `session.open` as its surface opens one: an MCP client's is
    /// [`Core::mcp_open`]'s, any other the plain open.
    pub(super) fn session_open_on(
        &self,
        surface: Surface,
        p: SessionOpenParams,
    ) -> Result<theseus_protocol::SessionInfo, RpcFailure> {
        match surface {
            Surface::Mcp => self.mcp_open(p),
            _ => self.session_open(p),
        }
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
