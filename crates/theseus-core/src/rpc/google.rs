//! `google.auth.begin`, `google.auth.finish`, `google.revoke` and
//! `google.status` (the Google design §2.2): the operator's, from the CLI on
//! this machine. Refused from Discord, from the web UI and from an MCP client,
//! as `aws.bootstrap` is. The CLI refuses them inside a Theseus job
//! (theseus-zmgb), a speed bump; L1 is the boundary. The paste, when there is
//! one, is read from the terminal and travels here in a param, never an
//! argument; none of these answers holds a token, a code or a secret.

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
