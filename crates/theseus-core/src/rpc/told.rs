//! `told.undo` (issue 57): the one door every Undo on a note of an act done
//! unasked goes through (`owner_note::told`).
//!
//! **Who.** A connection whose own peer reading (`peer_proc`, through
//! `Conn::answerer`: never a label the request supplies) names an agent, a
//! process with no terminal, or a job the daemon started, is refused first,
//! before anything is read, with the words any non-owner gets. Then the press
//! is one act of its own, judged by the general rule, whatever the undo calls
//! underneath: the owner, from a private place, as the daemon judges every
//! owner-only command. A process the peer reading takes for the owner's own
//! terminal counts as the owner, so a counted undo is not proof of a human
//! press.
//!
//! **Which note.** The note's full post id, as its own "Undo:" line names it;
//! anything else is not found.
//!
//! **Once.** A note already settled is refused. Then the door looks for the
//! undo's own lasting outcome (the order revoked, the item dropped): when it
//! is there, an earlier undo got that far and its receipt was lost, so the
//! note is marked and says so, and nothing runs again. Otherwise the undo
//! runs here, through the existing code of that undo, as the presser, and
//! only then is the note marked. Each undo's code writes its own frame, so
//! effect and receipt are two writes; this look is what keeps a press
//! once-only across a crash between them.
//!
//! **Out.** The answer is what the note says now. Every copy of a note's
//! words, a label or an error's words that leaves over the RPC passes the
//! exit's masking (`rpc_told_undo`); the stored note stays as written.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_protocol::{error_code, DiscordOrigin, StandingRevokeParams};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::owner_note::told::{self, Undo, Undone};

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

/// What an undo's code did.
enum Effect {
    /// It ran.
    Done,
    /// The act moved on before the press, so the undo no longer applies and
    /// nothing changed: the note's last line, and the words to say.
    Moved { line: String, message: String },
}

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

/// A press on a note already settled: nothing is done, and the note's words
/// as they now stand come back, so a note an earlier edit missed is mended.
fn settled(text: &str, u: &Undone) -> RpcFailure {
    let line = u.line();
    let message = match u.ran() {
        true => format!("it was already undone by {}: nothing was done", u.by),
        false => format!(
            "its undo was already settled ({}): nothing was done",
            line.trim_start_matches("↩️ ").trim_end_matches('.')
        ),
    };
    RpcFailure {
        code: error_code::INVALID_PARAMS,
        message,
        data: json!({"undone_by": u.by, "ran": u.ran(), "text": told::settled_text(text, &line)}),
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

/// Every string in `v`, masked by `mask`.
fn masked_json(v: Value, mask: &dyn Fn(&str) -> String) -> Value {
    match v {
        Value::String(s) => Value::String(mask(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| masked_json(x, mask)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| (k, masked_json(x, mask)))
                .collect(),
        ),
        other => other,
    }
}

impl Core {
    /// `told.undo` over the RPC: every copy that leaves passes the exit's
    /// masking, the note's words, labels and an error's words alike. The
    /// stored note stays as written.
    pub(super) async fn rpc_told_undo(
        &self,
        p: ToldUndoParams,
        conn: Conn<'_>,
    ) -> Result<ToldUndoResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        let mask = |s: &str| self.exits.text(s).0.into_owned();
        match self.told_undo(&p.post, &who).await {
            Ok(r) => Ok(ToldUndoResult {
                text: mask(&r.text),
                found: r.found,
            }),
            Err(e) => Err(RpcFailure {
                code: e.code,
                message: mask(&e.message),
                data: masked_json(e.data, &mask),
            }),
        }
    }

    /// The note `post`'s undo, as `who`'s act.
    pub async fn told_undo(
        &self,
        post: &str,
        who: &Answerer,
    ) -> Result<ToldUndoResult, RpcFailure> {
        let _one = UNDOING.lock().await;
        let what = format!("the undo of note {post}");
        // An agent's or a job's process, by the connection's own reading of
        // its peer: refused before any note is read, as a non-owner is.
        if who.agent.is_some() {
            let r = Refusal {
                who: who.who(),
                via: who.via(),
                why: format!("{} is not an owner", who.who()),
            };
            self.refused(Act::Told { what: &what }, who, &r)
                .map_err(RpcFailure::invalid)?;
            return Err(failure(r.into()));
        }
        self.judge_act(who, Act::Told { what: &what })
            .map_err(failure)?;
        let Some((text, undo)) = told::of(&self.kernel, post).map_err(RpcFailure::invalid)? else {
            return Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no note {post} with an undo"),
            ));
        };
        if let Some(u) = told::undone(&self.store, post).map_err(RpcFailure::invalid)? {
            return Err(settled(&text, &u));
        }
        if let Some(found) = self.undo_found(&undo)? {
            return self.receipt(post, who, &text, &told::found_line(found), true);
        }
        match self.undo_now(&undo, who) {
            Ok(Effect::Done) => {}
            Ok(Effect::Moved { line, message }) => {
                // Nothing changed: the note says so, and loses its Undo. A
                // mark that is not kept leaves the next press to say it again.
                let text = told::mark_undone(&self.store, post, &who.label, &line)
                    .ok()
                    .map(|()| told::settled_text(&text, &line));
                return Err(RpcFailure {
                    code: error_code::INVALID_PARAMS,
                    message,
                    data: json!({"moved_on": true, "text": text}),
                });
            }
            // Refused by the undo's own rule, before it did anything.
            Err(e) if e.code == error_code::REFUSED => return Err(e),
            // It may have got some or all of the way before it failed.
            Err(e) => {
                return match self.undo_found(&undo) {
                    Ok(Some(found)) => {
                        self.receipt(post, who, &text, &told::found_line(found), true)
                    }
                    _ => Err(unfinished(&undo, post, e)),
                };
            }
        }
        self.receipt(post, who, &text, &told::undone_line(&who.label), false)
    }

    /// The note `post` marked settled by `who`, ending with `line`, after
    /// its undo's effect.
    fn receipt(
        &self,
        post: &str,
        who: &Answerer,
        text: &str,
        line: &str,
        found: bool,
    ) -> Result<ToldUndoResult, RpcFailure> {
        #[cfg(test)]
        if FAULT_RECEIPT.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(unkept(anyhow::anyhow!("a planted fault")));
        }
        told::mark_undone(&self.store, post, &who.label, line).map_err(unkept)?;
        Ok(ToldUndoResult {
            text: told::settled_text(text, line),
            found,
        })
    }

    /// The undo's lasting outcome, when it is there now: what was found.
    fn undo_found(&self, undo: &Undo) -> Result<Option<&'static str>, RpcFailure> {
        Ok(match undo {
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
                .self_item_dropped(id)
                .map_err(RpcFailure::invalid)?
                .then_some("the item is dropped"),
            // A Google act's outcome is Google's, seen only by a call. A
            // press after a crash runs the restore again, which sets each
            // thing to what it was before the act: what is back stays back.
            Undo::GoogleTakeBack(_) => None,
            #[cfg(test)]
            Undo::Probe(_) => None,
        })
    }

    /// An undo, through the existing code that makes it, as `who`. A new
    /// undo is one arm here: an existing method's function, or an internal
    /// one when the undo has no method of its own.
    fn undo_now(&self, undo: &Undo, who: &Answerer) -> Result<Effect, RpcFailure> {
        match undo {
            Undo::StandingRevoke(id) => {
                let p = StandingRevokeParams {
                    id: id.clone(),
                    author: Some(who.label.clone()),
                    discord: who.discord.clone(),
                };
                self.standing_revoke(&p, who.clone()).map(|_| Effect::Done)
            }
            // A filing's undo, only while the item is still as filed.
            Undo::SelfDrop(id) => match self.self_drop_as_filed(id, who) {
                Ok(true) => Ok(Effect::Done),
                Ok(false) => {
                    let command = undo.command();
                    Ok(Effect::Moved {
                        line: told::moved_line(&command),
                        message: format!(
                            "The item has moved on since it was filed (it started, ran, was \
                             retried or set aside), so this Undo no longer applies, and nothing \
                             was changed. To drop it as it is now: `{command}`."
                        ),
                    })
                }
                Err(e) => Err(failure(e)),
            },
            Undo::GoogleTakeBack(id) => self.google_take_back(id).await.map(|()| Effect::Done),
            #[cfg(test)]
            Undo::Probe(_) => {
                PROBED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Effect::Done)
            }
        }
    }
}

#[cfg(test)]
#[path = "tests_told.rs"]
mod tests;
