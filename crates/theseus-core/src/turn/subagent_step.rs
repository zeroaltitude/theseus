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
    /// says the turn ended on a stop, which a stop that landed since its input arrived says too.
    pub(super) fn post_subagents(&self, t: &Turn<'_>, stopped: bool) -> Result<()> {
        let answers = self.tools.profile.take(t.tc.turn_id);
        if answers.is_empty() {
            return Ok(());
        }
        let stopped = stopped || self.stopped_since(t.tc.execution_id, t.arrived).is_some();
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
    /// to the session's place, with the turn's end marked on it.
    pub(super) fn post_unposted_answers(
        &self,
        session_id: &str,
        execution_id: &str,
        turn_id: &str,
        answers: Vec<theseus_protocol::SubagentAnswer>,
        ended: &str,
    ) {
        for answer in answers {
            let body = json!({
                "kind": "subagent",
                "turn_id": turn_id,
                "session_id": session_id,
                "answer": answer,
                "reply_to": null,
                "turn_ended": ended,
            });
            if let Err(e) = self.outbox.post_for(session_id, execution_id, body) {
                tracing::warn!(error = %format!("{e:#}"), "an answer of a turn that ended was not written");
            }
        }
    }
}
