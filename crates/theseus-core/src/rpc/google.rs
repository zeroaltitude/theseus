//! `google.auth.begin`, `google.auth.finish`, `google.revoke` and
//! `google.status` (the Google design §2.2): the operator's, from the CLI on
//! this machine. Refused from Discord, from the web UI and from an MCP client,
//! as `aws.bootstrap` is. The CLI refuses them inside a Theseus job
//! (theseus-zmgb), a speed bump; L1 is the boundary. The paste, when there is
//! one, is read from the terminal and travels here in a param, never an
//! argument; none of these answers holds a token, a code or a secret.
//!
//! `google.undo` (issue 57) takes back a tidy act Theseus did unasked: the
//! owner's, from a private place (a note's Undo, or `theseus google undo`),
//! judged as a routine's delete is. It runs only the calls the act's own
//! result recorded (`google::told`), each through `google.call` and each one
//! that asks nobody; anything that would ask is refused.

use std::sync::Arc;

use theseus_protocol::error_code;
use theseus_protocol::google::{
    GoogleAuthBeginParams, GoogleAuthBeginResult, GoogleAuthFinishParams, GoogleAuthFinishResult,
    GoogleRevokeParams, GoogleRevokeResult, GoogleStatus, GoogleStatusParams,
};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use crate::approval::{Answerer, Surface};
use crate::google::Google;

/// `google.undo`'s params: the act's result node, and who pressed, from where.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoogleUndoParams {
    pub id: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub discord: Option<theseus_protocol::DiscordOrigin>,
}

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
            method::GOOGLE_UNDO => {
                let p: GoogleUndoParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                reply(self.google_undo(&p.id, &who).await?)
            }
            method::CALENDAR_WINDOW => reply(self.calendar_window(parse(params)?, conn).await?),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }

    /// `google.undo`: the owner's, from a private place. Each call the act's
    /// result recorded runs through `google.call`, as the owner approved it;
    /// one whose plan would ask anybody is refused, and nothing after it runs.
    pub(crate) async fn google_undo(
        &self,
        id: &str,
        who: &Answerer,
    ) -> Result<serde_json::Value, RpcFailure> {
        let what = format!("the undo of {id}");
        let method = theseus_protocol::method::GOOGLE_UNDO;
        self.judge_act(
            who,
            Act::Google {
                method,
                what: &what,
            },
        )
        .map_err(refused)?;
        let node = self.store.get_node(id).map_err(RpcFailure::invalid)?;
        let Some(calls) = node.and_then(|(_, n)| crate::google::told::undo_calls(&n)) else {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                format!("{id} is no Google act Theseus can take back"),
            ));
        };
        self.google()?;
        let tool = self
            .tools
            .registry
            .get(crate::google::call::NAME)
            .cloned()
            .ok_or_else(|| {
                RpcFailure::new(error_code::INVALID_PARAMS, "google.call is not here")
            })?;
        let ctx = theseus_tools::ToolCtx {
            approved: true,
            ..Default::default()
        };
        let mut done = Vec::new();
        for call in &calls {
            let plan = tool
                .plan(call, &ctx)
                .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?;
            if plan.authority.is_some() {
                return Err(RpcFailure::new(
                    error_code::REFUSED,
                    format!("{}: it would ask, so an undo does not run it", plan.summary),
                ));
            }
            let ran = tool.run_async(call, &ctx).await;
            let meta = match &ran {
                Ok((out, _)) => &out.meta,
                Err(f) => &f.meta,
            };
            let also = meta["google_called_also"].as_array().into_iter().flatten();
            for row in also.chain(meta.get("google_called")) {
                self.google_row(&crate::fact::google::GoogleCalled { row });
            }
            match ran {
                Ok(_) => done.push(plan.summary),
                Err(f) => {
                    return Err(RpcFailure::new(
                        error_code::INVALID_PARAMS,
                        format!("{}: {}", plan.summary, f.message),
                    ))
                }
            }
        }
        Ok(serde_json::json!({"undone": id, "calls": done}))
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

/// A refused undo on the wire is `REFUSED`, with who and why; anything else
/// is invalid input.
fn refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<crate::approval::Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "an undo from {} does not count: {}. Nothing in Google was changed.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
