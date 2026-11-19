//! A subagent's answer, posted (issue 28): the words another profile gave a call of this turn
//! (`profile.ask`) are shown to the owner as a message of their own, in the place the turn's reply
//! goes to, in order BEFORE that reply.
//!
//! The owner's rule (2026-11-18, 23:50 MST): a subagent's answer comes back into the conversation
//! "as Theseus, with a subagent footer, and the appropriate model listed in the model part". The
//! record is the call's result node (`meta.subagent`, which the history, the inspector and the
//! terminal read); this is the outbox's side, for a place that is reached through the outbox
//! (Discord). It goes where `stage_reply` sends the reply (an answer's place, a thread's turn
//! spoken in the owner's DM, the session's place, where a wake was set), once: a post is staged
//! in the turn's own frame, keyed by the call's id, so a retry that finds the call settled does not
//! ask again and posts nothing twice. **A turn that was stopped or that failed posts it too**: the
//! answer was produced and paid for, and its live message is already in the place, so the post
//! edits it and its footer says the turn that asked was stopped, or then failed (`turn_ended`).
//!
//! It is taken once, at the one place every turn's post passes (`stage_turn_post`, just before
//! the reply), so a call run by the turn's own loop, one the owner's yes let a continuation run,
//! and any path a third turn takes, post alike. What a turn did not post is dropped at its end
//! (`profile_tool::Board::forget`).

use super::*;

impl TurnRunner {
    /// Stage the answers this turn's calls got, one post each, in the order they came. `stopped`
    /// says the turn ended on a stop; a stop that landed in the window before a turn that went on
    /// to end well does not mark its answers.
    pub(super) fn post_subagents(&self, t: &Turn<'_>, stopped: bool) -> Result<()> {
        let answers = self.tools.profile.take(t.tc.turn_id);
        if answers.is_empty() {
            return Ok(());
        }
        // The record says how the turn that asked ended, for the surfaces that draw the answer
        // from its result node (the history, the cockpit), where there is no place to post to too.
        if stopped {
            for a in &answers {
                t.tc.store
                    .defer(crate::profile_tool::ended_record(&a.call, "stopped")?)?;
            }
        }
        let Some(target) = t
            .answer_at
            .clone()
            .or_else(|| self.outbox.target(t.tc.session_id))
            .or_else(|| t.wake_target.clone())
        else {
            return Ok(());
        };
        for answer in answers {
            let mut body = json!({
                "kind": "subagent",
                "turn_id": t.tc.turn_id,
                "session_id": t.tc.session_id,
                "answer": answer,
                "reply_to": t.reply_to,
            });
            if stopped {
                body["turn_ended"] = json!("stopped");
            }
            let (post, records) =
                self.outbox
                    .stage(t.tc.session_id, t.tc.execution_id, &target, body)?;
            for r in records {
                t.tc.store.defer(r)?;
            }
            t.tc.posts.lock().unwrap().push(post);
        }
        Ok(())
    }

    /// The answers of a turn that ended in an error (a fault, a failure, a stop that aborted it):
    /// no frame of the turn's carries a post by now, so each is written as the failed notice is,
    /// with the turn's end marked on it, and where the turn's reply would have gone (`answer_at`,
    /// else the session's place). The failed notice itself stays at the session's place: it is the
    /// notice of a run of failures that a retry carries on, and a retry has no `answer_at`.
    pub(super) fn post_unposted_answers(
        &self,
        turn: (&str, &str, &str, Option<String>),
        answers: Vec<theseus_protocol::SubagentAnswer>,
        ended: &str,
    ) {
        let (session_id, execution_id, turn_id, answer_at) = turn;
        self.record_ended(&answers, ended);
        let Some(target) = answer_at.or_else(|| self.outbox.target(session_id)) else {
            return;
        };
        for answer in answers {
            let body = json!({
                "kind": "subagent",
                "turn_id": turn_id,
                "session_id": session_id,
                "answer": answer,
                "reply_to": null,
                "turn_ended": ended,
            });
            if let Err(e) = self.outbox.post(session_id, execution_id, &target, body) {
                tracing::warn!(error = %format!("{e:#}"), "an answer of a turn that ended was not written");
            }
        }
    }

    /// Write how the turn that asked ended for each answer, for the surfaces that draw the answer
    /// from its result node; best effort, as the post is.
    fn record_ended(&self, answers: &[theseus_protocol::SubagentAnswer], ended: &str) {
        for answer in answers {
            let written = crate::profile_tool::ended_record(&answer.call, ended)
                .and_then(|r| self.store.append(&[r]));
            if let Err(e) = written {
                tracing::warn!(error = %format!("{e:#}"), "an answer's turn end was not recorded");
            }
        }
    }
}
