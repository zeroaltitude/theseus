//! `google.auth.begin`, `google.auth.finish`, `google.revoke` and
//! `google.status` (the Google design §2.2): the operator's, from the CLI on
//! this machine. Refused from Discord, from the web UI and from an MCP client,
//! as `aws.bootstrap` is. The CLI refuses them inside a Theseus job
//! (theseus-zmgb), a speed bump; L1 is the boundary. The paste, when there is
//! one, is read from the terminal and travels here in a param, never an
//! argument; none of these answers holds a token, a code or a secret.
//!
//! [`crate::Core::google_take_back`] (issue 57) takes back a tidy act
//! Theseus did unasked, for a note's Undo once `told.undo` has judged the
//! press: it runs only the calls the act's own result recorded
//! (`google::told`), each through `google.call` and each one that asks
//! nobody; anything that would ask is refused.

use std::sync::Arc;

use theseus_protocol::error_code;
use theseus_protocol::google::{
    GoogleAuthBeginParams, GoogleAuthBeginResult, GoogleAuthFinishParams, GoogleAuthFinishResult,
    GoogleRevokeParams, GoogleRevokeResult, GoogleStatus, GoogleStatusParams,
};

use super::server::{Conn, RpcFailure};
use crate::approval::Surface;
use crate::google::Google;

fn cli_only(conn: &Conn<'_>, method: &str, verb: &str) -> Result<(), RpcFailure> {
    if conn.surface != Surface::Cli {
        return Err(RpcFailure::new(
            error_code::REFUSED,
            format!(
                "{method} is the operator's, from the CLI on this machine: theseus google {verb}"
            ),
        ));
    }
    Ok(())
}

impl crate::Core {
    /// The four methods, from `rpc_prefixed`'s one arm.
    pub(super) async fn rpc_google(
        self: &Arc<Self>,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        use super::server::{parse, reply};
        use theseus_protocol::method;
        match name {
            method::GOOGLE_AUTH_BEGIN => reply(self.google_auth_begin(parse(params)?, conn).await?),
            method::GOOGLE_AUTH_FINISH => {
                reply(self.google_auth_finish(parse(params)?, conn).await?)
            }
            method::GOOGLE_REVOKE => reply(self.google_revoke(parse(params)?, conn).await?),
            method::GOOGLE_STATUS => reply(self.google_status(parse(params)?, conn)?),
            method::CALENDAR_WINDOW => reply(self.calendar_window(parse(params)?, conn).await?),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }

    /// The told act whose result node is `id`, taken back: `told.undo`'s arm
    /// for [`crate::owner_note::told::Undo::GoogleTakeBack`], which has judged
    /// the press already. Each thing the act changed is put back exactly as it
    /// was before it (`google::take_back`), each tried; one that cannot be is
    /// said, with how many were put back.
    pub(crate) async fn google_take_back(&self, id: &str) -> Result<(), RpcFailure> {
        let node = self.store.get_node(id).map_err(RpcFailure::invalid)?;
        let Some(before) = node.and_then(|(_, n)| crate::google::told::undo_before(&n)) else {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                format!("{id} is no Google act Theseus can take back"),
            ));
        };
        let g = self.google()?;
        let mut rows = Vec::new();
        let tools = (crate::google::mail::LABEL, crate::google::drive::SHARE);
        let p = crate::google::take_back::restore(&g, tools, &before, &mut rows).await;
        for row in &rows {
            self.google_row(&crate::fact::google::GoogleCalled { row });
        }
        match p.whole() {
            true => Ok(()),
            false => Err(RpcFailure::new(error_code::INVALID_PARAMS, p.words())),
        }
    }

    fn google(&self) -> Result<Arc<Google>, RpcFailure> {
        self.tools.google.clone().ok_or_else(|| {
            RpcFailure::new(
                error_code::INVALID_PARAMS,
                "no Google account is bound: the config has no [google] account",
            )
        })
    }

    pub(super) async fn google_auth_begin(
        &self,
        p: GoogleAuthBeginParams,
        conn: Conn<'_>,
    ) -> Result<GoogleAuthBeginResult, RpcFailure> {
        cli_only(&conn, "google.auth.begin", "auth")?;
        self.google()?
            .auth_begin(p.manual)
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e))
    }

    pub(super) async fn google_auth_finish(
        &self,
        p: GoogleAuthFinishParams,
        conn: Conn<'_>,
    ) -> Result<GoogleAuthFinishResult, RpcFailure> {
        cli_only(&conn, "google.auth.finish", "auth")?;
        let g = self.google()?;
        let r = g
            .auth_finish(&p, self.cfg.op_token_file.as_deref())
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e))?;
        if r.state == "done" {
            let scopes: Vec<String> = g.check_now().granted.into_iter().collect();
            self.google_authorized(g.account(), &scopes, &r.store);
            tracing::info!(store = %r.store, "google: authorized");
        }
        Ok(r)
    }

    pub(super) async fn google_revoke(
        &self,
        _p: GoogleRevokeParams,
        conn: Conn<'_>,
    ) -> Result<GoogleRevokeResult, RpcFailure> {
        cli_only(&conn, "google.revoke", "revoke")?;
        let g = self.google()?;
        let r = g.revoke(self.cfg.op_token_file.as_deref()).await;
        self.google_revoked(g.account(), r.revoked_at_google, r.cleared);
        Ok(r)
    }

    pub(super) fn google_status(
        &self,
        _p: GoogleStatusParams,
        conn: Conn<'_>,
    ) -> Result<GoogleStatus, RpcFailure> {
        cli_only(&conn, "google.status", "status")?;
        Ok(self.google()?.status())
    }
}
