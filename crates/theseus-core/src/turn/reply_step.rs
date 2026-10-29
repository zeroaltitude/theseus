//! The turn's reply as an outbox post (theseus-q4v), apart from `turn.rs` for
//! the shape budget's file ceiling: each loop's text and its footer, to where
//! the turn was answered (`answer_at`, a thread's turn spoken in the owner's
//! DM, theseus-sjzm), else its session's place, else where a wake was set.

use super::*;

impl TurnRunner {
    /// The turn's reply as an outbox post, when its session posts somewhere
    /// (theseus-q4v): each loop's text, by node, and what its footer says.
    /// It rides in the frame that ends the turn, so a turn that ended has its
    /// reply written, whether or not a binding is connected.
    pub(super) fn stage_reply(&self, t: &Turn<'_>, result: &TurnSubmitResult) -> Result<()> {
        // What the reply answers: the wakes and reports this turn took. The
        // retry of a turn that failed before its reply took none, since the
        // first took them, and their nodes are still unanswered in the
        // session: it frames its reply from those, and finds the target the
        // take kept (theseus-4lx).
        let (mut wakes, mut reports) = (t.wakes.clone(), t.woke_by.clone());
        let mut retried = false;
        if wakes.is_empty() && reports.is_empty() {
            (wakes, reports) = Self::unread_relays(t)?;
            retried = !wakes.is_empty() || !reports.is_empty();
        }
        // A wake's turn in a session its place has moved on from (`/new`)
        // still answers where the wake was set (DD8).
        let Some(target) = t
            .answer_at
            .clone()
            .or_else(|| self.outbox.target(t.tc.session_id))
            .or_else(|| t.wake_target.clone())
            .or_else(|| {
                retried
                    .then(|| self.outbox.wake_target(t.tc.session_id))
                    .flatten()
            })
        else {
            return Ok(());
        };
        // Where Jev filed the session, when its judgment had landed by now
        // (theseus-xzxqv); else it comes as a notice when it does.
        let filed = self.judge.take_filed(crate::judge::offer::Waiting {
            turn: t.tc.turn_id,
            session: t.tc.session_id,
            execution: t.tc.execution_id,
            target: &target,
        });
        let footer = TurnSubmitResult {
            output: String::new(),
            trace: None,
            later: None,
            filed,
            ..result.clone()
        };
        let mut body = json!({
            "kind": "reply",
            "turn_id": result.turn_id,
            "loops": t.said,
            "result": footer,
            "reply_to": t.reply_to,
        });
        if !wakes.is_empty() {
            body["wakes"] = json!(wakes);
        }
        if !reports.is_empty() {
            body["reports"] = json!(reports);
        }
        if let Some(brief) = self.offer_for(t, result, &target) {
            body["offer"] = json!({"brief": brief});
        }
        let (post, records) =
            self.outbox
                .stage(t.tc.session_id, t.tc.execution_id, &target, body)?;
        for r in records {
            t.tc.store.defer(r)?;
        }
        t.tc.posts.lock().unwrap().push(post);
        Ok(())
    }

    /// The brief of the task this reply offers (theseus-bh3t.5): `classify.v3`
    /// found the owner's message to be work for later, at the act bar, and
    /// the batch had landed by now. Not on a detour (trivial, quick, run_tool or commentary), a task's
    /// turn, a message not alone with the owner, or a turn that made a task
    /// itself.
    fn offer_for(&self, t: &Turn<'_>, result: &TurnSubmitResult, target: &str) -> Option<String> {
        let detour = result
            .route
            .as_ref()
            .and_then(|r| r.mode.as_deref())
            .is_some_and(crate::config::routing::is_detour);
        if detour || t.tc.task.is_some() || !crate::capture::owner_alone(t.tc.class, Some(target)) {
            return None;
        }
        let brief = self.judge.take_offer(crate::judge::offer::Waiting {
            turn: t.tc.turn_id,
            session: t.tc.session_id,
            execution: t.tc.execution_id,
            target,
        })?;
        (crate::judge::offer::made_a_task(t.tc.store, t.tc.session_id, t.tc.turn_id) == Some(false))
            .then_some(brief)
    }

    /// The wakes and reports an earlier attempt took, which this turn answers
    /// (theseus-4lx): the harness's wake and task-report nodes written by a
    /// turn other than this one, after the last answer the session gave. Each
    /// is the line the reply's header names: the node's own text for a wake,
    /// and `task:<short>`'s report line, as the turn that took it would have
    /// said.
    fn unread_relays(t: &Turn<'_>) -> Result<(Vec<Value>, Vec<Value>)> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let (mut wakes, mut reports) = (Vec::new(), Vec::new());
        for (_, n) in nodes.iter().rev() {
            if n.turn_id.as_deref() == Some(t.tc.turn_id) {
                continue;
            }
            match &n.body {
                // An answer: what comes before it was read.
                Body::AssistantMessage { blocks, .. }
                    if crate::provider::tool_uses_in(blocks).is_empty() =>
                {
                    break
                }
                Body::UserMessage { text, .. }
                    if matches!(n.origin, crate::node::Origin::Harness) =>
                {
                    let author = n.author.as_deref().unwrap_or_default();
                    if author.starts_with("wake:") {
                        wakes.push(json!({"text": text}));
                    } else if let Some(short) = author.strip_prefix("task:") {
                        reports
                            .push(json!({"short": short, "text": crate::task::woke_line(short)}));
                    }
                }
                _ => {}
            }
        }
        wakes.reverse();
        reports.reverse();
        Ok((wakes, reports))
    }
}
