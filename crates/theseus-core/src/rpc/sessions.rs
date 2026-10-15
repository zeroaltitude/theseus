//! `session.retire` and `session.reopen` (theseus-emqx): the owner's acts on
//! a session's state, judged as the core judges every owner's act (the
//! owner, from a private place; `crate::succession`), and refused by the CLI
//! inside a job. Each answers the session as `session.list` shows it. And
//! `session.nudge`, a session's own switch of the stop nudge (theseus-fh0u).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::nudge::{SessionNudgeParams, SessionNudgeResult};
use theseus_protocol::sessions::{SessionReopenParams, SessionRetireParams};
use theseus_protocol::SessionInfo;
use theseus_protocol::{error_code, method};

use super::server::{or_empty, route, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;
use crate::session::SessionRecord;

/// The methods routed here: the list, the owner's two acts, a person's
/// first keystroke (theseus-tnky), the stop nudge's switch (theseus-fh0u),
/// a fork (theseus-2qzx), a rewind's two (theseus-23rk), and a conversation's mode (theseus-lyxy).
pub(super) const OWN: [&str; 9] = [
    method::SESSION_LIST,
    method::SESSION_RETIRE,
    method::SESSION_REOPEN,
    method::SESSION_TYPING,
    method::SESSION_NUDGE,
    method::SESSION_FORK,
    method::FILES_CHANGES,
    method::FILES_RESTORE,
    method::SESSION_MODE,
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
            method::SESSION_NUDGE => route(params, |p| self.session_nudge(p)),
            method::SESSION_FORK => route(params, |p| self.session_fork(p, conn.surface)),
            method::FILES_CHANGES => route(params, |p| self.files_changes(p, conn)),
            method::FILES_RESTORE => route(params, |p| self.files_restore(p, conn)),
            method::SESSION_MODE => route(params, |p| self.session_mode(p, conn)),
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

    /// `session.nudge` (theseus-fh0u): the stop nudge off, or back on, for
    /// one session, as META `stop_nudge.<session>`, which its turns read at
    /// their stop. Anyone's: it only lowers what `[judge] stop_nudge` allows.
    pub(crate) fn session_nudge(
        &self,
        p: SessionNudgeParams,
    ) -> Result<SessionNudgeResult, RpcFailure> {
        if self
            .store
            .get_session::<SessionRecord>(&p.session_id)?
            .is_none()
        {
            return Err(RpcFailure::invalid(anyhow::anyhow!(
                "no session {}",
                p.session_id
            )));
        }
        let key = crate::turn::nudge_step::session_key(&p.session_id);
        self.store.put_meta(&key, &json!({"on": p.on}))?;
        let judge = self.runner.judge.config();
        Ok(SessionNudgeResult {
            session_id: p.session_id,
            on: p.on,
            daemon_on: judge.enabled && judge.stop_nudge,
        })
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
pub(super) fn refused(e: anyhow::Error) -> RpcFailure {
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
