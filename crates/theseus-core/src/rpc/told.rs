//! `told.undo` (issue 57): the one door every Undo on a note of an act done
//! unasked goes through (`owner_note::told`). It is one act of its own,
//! judged by the general rule, whatever the undo calls underneath: the owner,
//! from a private place, as the daemon judges every owner-only command. A
//! local process on the owner's socket counts as the owner's terminal, so a
//! counted undo is not proof of a human press.
//!
//! A note already undone is refused. Then the door looks for the undo's own
//! lasting outcome (the wake no longer pending, the order revoked, the
//! routine gone, the item out of the backlog): when it is there, an earlier
//! undo got that far and its receipt was lost, so the note is marked and says
//! so, and nothing runs again. Otherwise the undo runs here, through the
//! existing code of that undo, as the presser, and only then is the note
//! marked undone. Each undo's code writes its own frame, so effect and
//! receipt are two writes; this look is what keeps a press once-only across
//! a crash between them. The answer is what the note says now.

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
    /// The undo's outcome was already there (an earlier press got that far):
    /// nothing ran this time; the note was only marked.
    #[serde(default)]
    pub found: bool,
}

/// One undo at a time, so two quick presses undo once.
static UNDOING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// How many times the tests' undo with no method ran.
#[cfg(test)]
pub(crate) static PROBED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A fault planted between an undo's effect and its receipt (tests only).
#[cfg(test)]
pub(crate) static FAULT_RECEIPT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// An undo that failed after it may have begun: never "nothing was undone".
fn unfinished(undo: &Undo, post: &str, e: RpcFailure) -> RpcFailure {
    RpcFailure {
        code: e.code,
        message: format!(
            "The undo did not finish: {}. The note is not marked undone, and part of it may \
             already be done. Press Undo again to finish it, or use `{}`.",
            e.message,
            told::filled(&undo.command(), post)
        ),
        data: json!({"unfinished": true, "cause": e.message}),
    }
}

/// An undo done whose receipt was not written.
fn unkept(e: anyhow::Error) -> RpcFailure {
    RpcFailure {
        code: error_code::INTERNAL,
        message: format!(
            "The undo was done, but the note's record of it was not kept: {e:#}. Press Undo \
             again: it will find the undo done and mark the note, and run nothing twice."
        ),
        data: json!({"done": true}),
    }
}

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
    pub async fn told_undo(
        &self,
        post: &str,
        who: &Answerer,
    ) -> Result<ToldUndoResult, RpcFailure> {
        let _one = UNDOING.lock().await;
        let what = format!("the undo of note {post}");
        self.judge_act(who, Act::Told { what: &what })
            .map_err(failure)?;
        // Its post id, or the end of one (`theseus told undo`).
        let found = told::resolve(&self.kernel, post).map_err(RpcFailure::invalid)?;
        let not_found = || {
            RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no note {post} with an undo"),
            )
        };
        let post = &found.ok_or_else(not_found)?;
        let Some((text, undo)) = told::of(&self.kernel, post).map_err(RpcFailure::invalid)? else {
            return Err(not_found());
        };
        if let Some(u) = told::undone(&self.store, post).map_err(RpcFailure::invalid)? {
            return Err(RpcFailure {
                code: error_code::INVALID_PARAMS,
                message: format!("it was already undone by {}: nothing was done", u.by),
                data: json!({"undone_by": u.by, "text": told::undone_text(&text, &u.by)}),
            });
        }
        if let Some(found) = self.undo_found(&undo)? {
            return self.receipt(post, who, told::found_text(&text, found), true);
        }
        match self.undo_now(&undo, who).await {
            Ok(()) => {}
            // Refused by the undo's own rule, before it did anything.
            Err(e) if e.code == error_code::REFUSED => return Err(e),
            // It may have got some or all of the way before it failed.
            Err(e) => {
                return match self.undo_found(&undo) {
                    Ok(Some(found)) => {
                        self.receipt(post, who, told::found_text(&text, found), true)
                    }
                    _ => Err(unfinished(&undo, post, e)),
                };
            }
        }
        self.receipt(post, who, told::undone_text(&text, &who.label), false)
    }

    /// The note `post` marked undone by `who`, after its undo's effect.
    fn receipt(
        &self,
        post: &str,
        who: &Answerer,
        text: String,
        found: bool,
    ) -> Result<ToldUndoResult, RpcFailure> {
        #[cfg(test)]
        if FAULT_RECEIPT.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(unkept(anyhow::anyhow!("a planted fault")));
        }
        told::mark_undone(&self.store, post, &who.label).map_err(unkept)?;
        Ok(ToldUndoResult { text, found })
    }

    /// The undo's lasting outcome, when it is there now: what was found.
    fn undo_found(&self, undo: &Undo) -> Result<Option<&'static str>, RpcFailure> {
        Ok(match undo {
            Undo::RoutineDelete(id) => {
                let all = self.routine_list()?.routines;
                (!all.iter().any(|r| r.id == *id)).then_some("the routine is gone")
            }
            Undo::WakeCancel(id) => {
                let pending = self.kernel.pending_wakes().map_err(RpcFailure::invalid)?;
                (!pending.iter().any(|(_, w)| w.id == *id))
                    .then_some("the wake is no longer pending")
            }
            Undo::StandingRevoke(id) => {
                let all = self
                    .runner
                    .standing
                    .all(&self.store)
                    .map_err(RpcFailure::invalid)?;
                all.iter()
                    .any(|r| r.ends.as_deref() == Some(id.as_str()))
                    .then_some("the standing order is revoked")
            }
            Undo::SelfDrop(id) => self
                .self_item_closed(id)
                .map_err(RpcFailure::invalid)?
                .then_some("the item has left the backlog"),
            Undo::TaskUndo { task, version } => {
                crate::task_graph::undo::undone(&self.store, task, *version)
                    .map_err(RpcFailure::invalid)?
                    .then_some("the task is back as it was")
            }
            #[cfg(test)]
            Undo::Probe(_) => None,
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
            // An internal one: the task's record before the change, written back.
            Undo::TaskUndo { task, version } => self.task_undo_as(task, *version, who),
            #[cfg(test)]
            Undo::Probe(_) => {
                PROBED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "tests_told.rs"]
mod tests;
