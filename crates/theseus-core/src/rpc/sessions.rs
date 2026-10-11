//! `session.retire` and `session.reopen` (theseus-emqx): the owner's acts on
//! a session's state, judged as the core judges every owner's act (the
//! owner, from a private place; `crate::succession`), and refused by the CLI
//! inside a job. Each answers the session as `session.list` shows it.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::sessions::{SessionReopenParams, SessionRetireParams};
use theseus_protocol::SessionInfo;
use theseus_protocol::{error_code, method};

use super::server::{or_empty, route, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;
use crate::session::SessionRecord;

/// The methods routed here: the list, the owner's two acts, and a person's
/// first keystroke (theseus-tnky).
pub(super) const OWN: [&str; 4] = [
    method::SESSION_LIST,
    method::SESSION_RETIRE,
    method::SESSION_REOPEN,
    method::SESSION_TYPING,
];

impl Core {
    pub(super) fn rpc_sessions(
        self: &Arc<Self>,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match m {
            // Its filter is optional: no params lists every session.
            method::SESSION_LIST => route(or_empty(params), |p| self.session_list_of(p)),
            method::SESSION_RETIRE => route(params, |p| self.rpc_session_retire(p, conn)),
            method::SESSION_TYPING => route(params, |p| Ok(self.session_typing(p, conn))),
            _ => route(params, |p| self.rpc_session_reopen(p, conn)),
        }
    }

    fn rpc_session_retire(
        &self,
        p: SessionRetireParams,
        conn: Conn<'_>,
    ) -> Result<SessionInfo, RpcFailure> {
        let who = conn.answerer(None, None);
        let rec = self.retire_session(&p.session_id, &who).map_err(refused)?;
        Ok(self.shown(&rec))
    }

    fn rpc_session_reopen(
        &self,
        p: SessionReopenParams,
        conn: Conn<'_>,
    ) -> Result<SessionInfo, RpcFailure> {
        let who = conn.answerer(None, None);
        let rec = self.reopen_session(&p.session_id, &who).map_err(refused)?;
        Ok(self.shown(&rec))
    }

    /// One session as `session.list` shows it.
    fn shown(&self, rec: &SessionRecord) -> SessionInfo {
        let pending = self.pending_by_execution(
            &self
                .kernel
                .pending_confirms()
                .unwrap_or_default()
                .into_iter()
                .filter(|a| a.session_id == rec.session_id)
                .collect::<Vec<_>>(),
            None,
        );
        self.session_info(rec, &pending)
    }
}

/// A refusal as the owner's acts answer one; any other error is the params'.
fn refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a session's state from {} does not change: {}. The session is as it was.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
