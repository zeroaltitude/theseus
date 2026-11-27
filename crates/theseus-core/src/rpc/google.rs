//! `google.auth.begin`, `google.auth.finish`, `google.revoke` and
//! `google.status` (the Google design §2.2): the operator's, from the CLI on
//! this machine; and (issue 59) `auth.*` and `revoke` also from the owner's own
//! press of a control in a private place on Discord (`owner_press`). Refused
//! from Discord otherwise, from the web UI and from an MCP client, as
//! `aws.bootstrap` is. The CLI refuses them inside a Theseus job
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

/// What a press from Discord is told when it is not the owner's own, in a private place: fixed
/// words, with the place rule's own reason after them.
pub const NOT_YOURS: &str = "Google's consent from Discord is the owner's own press, in a private \
                             place (a direct message with Theseus, or a channel bound private)";

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

/// The fixed categories a Discord press is told when Google's consent fails (issue 59), chosen by
/// how the daemon's words begin: nothing that was pasted and none of Google's own text leaves the
/// daemon toward Discord. The binding answers each with its own fixed words.
pub const DISCORD_FAILURES: [&str; 7] = [
    "no sign-in is open",
    "the redirect was taken",
    "the redirect's state is not this consent's",
    "the address holds no code",
    "that sign-in was for a different account than `[google] account`",
    "Google named no account",
    "Google granted none",
];

/// What any other failure is on the Discord surface.
pub const DISCORD_REFUSED: &str = "Google refused the consent";

/// A failure's message for the surface that asked: the CLI's words exactly as before; for a
/// Discord press, one of the fixed categories.
fn failure_for(surface: Surface, message: String) -> String {
    if surface != Surface::Discord {
        return message;
    }
    DISCORD_FAILURES
        .iter()
        .find(|f| message.starts_with(*f))
        .copied()
        .unwrap_or(DISCORD_REFUSED)
        .to_string()
}

impl crate::Core {
    /// The CLI; or the owner's own press in a private place on Discord; else `cli_only`'s refusal.
    fn google_admitted(
        &self,
        conn: &Conn<'_>,
        method: &str,
        verb: &str,
        author: Option<String>,
        discord: Option<theseus_protocol::DiscordOrigin>,
    ) -> Result<(), RpcFailure> {
        match self.owner_press(conn, author, discord) {
            Some(Ok(())) => Ok(()),
            Some(Err(why)) => Err(RpcFailure::new(
                error_code::REFUSED,
                format!("{NOT_YOURS}: {why}"),
            )),
            None => cli_only(conn, method, verb),
        }
    }

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
        self.google_admitted(
            &conn,
            "google.auth.begin",
            "auth",
            p.author.clone(),
            p.discord.clone(),
        )?;
        let surface = conn.surface;
        self.google()?
            .auth_begin(p.manual)
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, failure_for(surface, e)))
    }

    pub(super) async fn google_auth_finish(
        &self,
        p: GoogleAuthFinishParams,
        conn: Conn<'_>,
    ) -> Result<GoogleAuthFinishResult, RpcFailure> {
        self.google_admitted(
            &conn,
            "google.auth.finish",
            "auth",
            p.author.clone(),
            p.discord.clone(),
        )?;
        let g = self.google()?;
        let surface = conn.surface;
        let r = g
            .auth_finish(&p, self.cfg.op_token_file.as_deref())
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, failure_for(surface, e)))?;
        if r.state == "done" {
            let scopes: Vec<String> = g.check_now().granted.into_iter().collect();
            self.google_authorized(g.account(), &scopes, &r.store);
            tracing::info!(store = %r.store, "google: authorized");
        }
        Ok(r)
    }

    pub(super) async fn google_revoke(
        &self,
        p: GoogleRevokeParams,
        conn: Conn<'_>,
    ) -> Result<GoogleRevokeResult, RpcFailure> {
        self.google_admitted(&conn, "google.revoke", "revoke", p.author, p.discord)?;
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
