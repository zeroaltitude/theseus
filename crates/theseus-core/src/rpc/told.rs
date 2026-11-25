//! `told.undo` (issue 57): the one door every Undo on a note of an act done
//! unasked goes through (`owner_note::told`). It is one act of its own,
//! judged by the general rule (the owner, from a private place), whatever the
//! undo calls underneath; a note already undone is refused; then the note's
//! undo runs here, through the existing code of that undo, as the presser,
//! and the note is marked undone. The answer is what the note says now.

use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::routines::RoutineNameParams;
use theseus_protocol::{error_code, DiscordOrigin, StandingRevokeParams};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::owner_note::told::{self, Undo};

/// `told.undo`'s params: the note's post id, and who pressed, from where.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToldUndoParams {
    pub post: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<DiscordOrigin>,
}

/// `told.undo`'s answer: the note's words now that it is undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToldUndoResult {
    pub text: String,
}

/// One undo at a time, so two quick presses undo once.
static UNDOING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "an undo from {} does not count: {}. Nothing was undone.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

impl Core {
    pub(super) async fn rpc_told_undo(
        &self,
        p: ToldUndoParams,
        conn: Conn<'_>,
    ) -> Result<ToldUndoResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.told_undo(&p.post, &who).await
    }

    /// The note `post`'s undo, as `who`'s act.
    pub async fn told_undo(&self, post: &str, who: &Answerer) -> Result<ToldUndoResult, RpcFailure> {
        let _one = UNDOING.lock().await;
        let what = format!("the undo of note {post}");
        self.judge_act(who, Act::Told { what: &what })
            .map_err(failure)?;
        let Some((text, undo)) = told::of(&self.kernel, post).map_err(RpcFailure::invalid)? else {
            return Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no note {post} with an undo"),
            ));
        };
        if let Some(u) = told::undone(&self.store, post).map_err(RpcFailure::invalid)? {
            return Err(RpcFailure {
                code: error_code::INVALID_PARAMS,
                message: format!("it was already undone by {}: nothing was done", u.by),
                data: json!({"undone_by": u.by}),
            });
        }
        self.undo_now(&undo, who).await?;
        told::mark_undone(&self.store, post, &who.label)?;
        Ok(ToldUndoResult {
            text: told::undone_text(&text, &who.label),
        })
    }

    /// An undo, through the existing code that makes it, as `who`. A new
    /// undo is one arm here: an existing method's function, or an internal
    /// one when the undo has no method of its own.
    async fn undo_now(&self, undo: &Undo, who: &Answerer) -> Result<(), RpcFailure> {
        let (author, discord) = (Some(who.label.clone()), who.discord.clone());
        match undo {
            Undo::RoutineDelete(id) => {
                let p = RoutineNameParams {
                    id: id.clone(),
                    author,
                    discord,
                };
                self.routine_delete(&p, who.clone()).await.map(drop)
            }
            Undo::WakeCancel(id) => self
                .wake_cancel_by(id, &who.label)
                .map(drop)
                .map_err(failure),
            Undo::StandingRevoke(id) => {
                let p = StandingRevokeParams {
                    id: id.clone(),
                    author,
                    discord,
                };
                self.standing_revoke(&p, who.clone()).map(drop)
            }
            Undo::SelfDrop(id) => {
                let p = theseus_protocol::rsi::SelfDropParams {
                    item: id.clone(),
                    author,
                    discord,
                    ..Default::default()
                };
                self.self_drop(&p, who).map(drop).map_err(failure)
            }
        }
    }
}
