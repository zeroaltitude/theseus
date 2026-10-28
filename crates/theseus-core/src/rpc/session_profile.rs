//! `session.profile` (theseus-jid7t.2): the model a session's turns run on,
//! held. It does what `turn.submit`'s per-turn `profile` does, from the next
//! turn on, and stays until it is let go.
//!
//! - **The hold** is the META key `session.profile.<session>`: the profile, and
//!   the target the session last ran on when it was first set (`was`), which a
//!   revert puts back so a continuation returns to where it ran before.
//!   `null` is the revert. One frame with its `session.profile_set` row.
//! - **Precedence**, for a turn: its own `profile` (not a pane's carried one),
//!   then this hold (`sticky_named`, in `turn.submit`; `target_as_ran`, for
//!   the continuations, wakes and tasks' turns), then the last turn's and the
//!   place's and the live one. Routing may still move a turn that was not
//!   pinned; the floor moves a refused base as it moves any, `moved_to` saying
//!   so (`turn/data_step.rs`). A turn fixes its model once it starts, so a
//!   running task takes the hold at its next turn, never mid-call.
//! - **The refusal** is the floor's own question (`TurnRunner::profile_refusal`): a
//!   profile that does not resolve, or of restricted origin for a session that
//!   carries a label that keeps one off. Nothing is written and the answer says
//!   why.
//! - **Authority** is `session.retire`'s: the owner, from a private place.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::ops::{SessionProfileParams, SessionProfileResult};
use theseus_protocol::{method, TurnSubmitParams};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::session::TargetRef;
use crate::turn::{Target, TurnRunner};

const KEY: &str = "session.profile.";

/// The hold as stored; `null` once let go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Sticky {
    profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    was: Option<TargetRef>,
}

impl TurnRunner {
    /// The profile `session_id` is held to, when it is still configured.
    pub(crate) fn sticky_profile(&self, session_id: &str) -> Option<String> {
        self.sticky(session_id)
            .map(|s| s.profile)
            .filter(|p| self.cfg.all_profiles().contains_key(p))
    }

    fn sticky(&self, session_id: &str) -> Option<Sticky> {
        let key = format!("{KEY}{session_id}");
        match self.store.get_meta::<Option<Sticky>>(&key) {
            Ok(held) => held.flatten(),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), session_id, "a session's held profile was not read");
                None
            }
        }
    }

    /// What a continuation runs on when its session is held to a profile.
    pub(crate) fn sticky_target(&self, session_id: &str, live: &str) -> Option<Target> {
        let profile = self.sticky_profile(session_id)?;
        self.resolve_target(live, Some(&profile), None, None).ok()
    }

    /// The profile `turn.submit` names for a turn: its own, else the hold, else
    /// `named`, which carries the last turn's.
    pub(crate) fn sticky_named<'a>(
        &self,
        session_id: &str,
        p: &TurnSubmitParams,
        named: Option<&'a str>,
        held: &'a mut Option<String>,
    ) -> Option<&'a str> {
        if p.profile.is_some() && !p.carried {
            return named;
        }
        *held = self.sticky_profile(session_id);
        held.as_deref().or(named)
    }
}

impl Core {
    pub(super) fn rpc_session_profile(
        self: &Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: SessionProfileParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.session_set_profile(&p, &who)
                .map_err(super::sessions::refused)
        })
    }

    /// Hold `p.session_id` to `p.profile` (none lets go) for `who`.
    pub fn session_set_profile(
        &self,
        p: &SessionProfileParams,
        who: &Answerer,
    ) -> anyhow::Result<SessionProfileResult> {
        let what = format!("Choosing the model of session {}", p.session_id);
        self.judge_act(
            who,
            Act::Session {
                method: method::SESSION_PROFILE,
                what: &what,
            },
        )?;
        let sid = p.session_id.as_str();
        let rec = self
            .store
            .get_session::<crate::session::SessionRecord>(sid)?
            .ok_or_else(|| anyhow::anyhow!("no session {sid}"))?;
        // A hold on a retired session is refused; a let-go always goes, so a hold
        // can be cleared.
        anyhow::ensure!(
            p.profile.is_none() || rec.retired.is_none(),
            "{sid} is retired: `theseus sessions reopen` it first"
        );
        let held = self.runner.sticky(sid);
        let previous = held.as_ref().map(|s| s.profile.clone());
        let mut done = SessionProfileResult {
            session_id: sid.to_string(),
            profile: previous.clone(),
            previous: previous.clone(),
            refused: None,
            note: None,
        };
        let next = match p.profile.as_deref() {
            Some(name) => {
                let mark = crate::content_wait::mark_of(&self.store, sid);
                if let Some(why) = self.runner.profile_refusal(mark, name) {
                    done.refused = Some(why);
                    return Ok(done);
                }
                done.note = self.runner.per_turn_note(mark, name);
                Some(Sticky {
                    profile: name.to_string(),
                    was: held
                        .as_ref()
                        .map_or_else(|| rec.last_target.clone(), |s| s.was.clone()),
                })
            }
            None => None,
        };
        done.profile = next.as_ref().map(|s| s.profile.clone());
        if done.profile == previous {
            return Ok(done);
        }
        let by = who.label.as_str();
        let set = fact::ops::SessionProfileSet {
            session_id: sid,
            from: previous.as_deref(),
            to: done.profile.as_deref(),
            by,
        };
        let mut frame = vec![
            NewRecord::json(kinds::META, Some(&format!("{KEY}{sid}")), &next)?,
            fact::row(&set, Some(sid), None)?,
        ];
        // A let-go puts back the target the session ran on before it was held.
        let restore = next.is_none().then(|| held.and_then(|s| s.was)).flatten();
        self.store.with_session(sid, |mut rec| {
            if let Some(was) = restore {
                rec.last_target = Some(was);
                frame.push(NewRecord::json(kinds::SESSION, Some(sid), &rec)?);
            }
            self.store.append(&frame)?;
            Ok(())
        })?;
        self.rec_for(sid).announce(&set);
        Ok(done)
    }
}

#[cfg(test)]
#[path = "tests_session_profile.rs"]
mod tests;
