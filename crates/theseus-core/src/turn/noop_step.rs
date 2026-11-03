//! A continuation that made no model call posts no reply (theseus-posww). A
//! turn that ends `nothing_new` or `awaiting_confirm` with no loop has no
//! answer to give, and its footer alone looked like one: on 2026-10-11 a
//! person asked for a retry of a failed turn and was sent `0 loops · $0.0000
//! this reply · stopped: nothing_new`, which said nothing of why. Now such a
//! turn posts nothing, unless a person is waiting on it (the session is in a
//! run of failed turns, and this continuation is its retry): then one plain
//! line says why nothing was done and what they can do.

use super::*;

/// The stop reasons of a turn that looked at the session and found nothing for
/// the model to do.
const NO_CALL: [&str; 2] = ["nothing_new", "awaiting_confirm"];

impl TurnRunner {
    /// The turn's post: its reply, or for a continuation that made no model
    /// call, a line to the person waiting on it, or nothing.
    pub(super) fn stage_turn_post(
        &self,
        t: &Turn<'_>,
        result: &TurnSubmitResult,
        failing: Option<&crate::session::Failing>,
    ) -> Result<()> {
        let no_call = t.continuation
            && result.loops == 0
            && NO_CALL.contains(&result.stop_reason.as_str())
            && t.wakes.is_empty()
            && t.woke_by.is_empty();
        if !no_call {
            return self.stage_reply(t, result);
        }
        let Some(failing) = failing else {
            return Ok(());
        };
        let Some(target) = self.outbox.target(t.tc.session_id) else {
            return Ok(());
        };
        let (post, records) = self.outbox.stage(
            t.tc.session_id,
            t.tc.execution_id,
            &target,
            json!({"kind": "notice", "text": nothing_to_retry(&result.stop_reason, failing)}),
        )?;
        for r in records {
            t.tc.store.defer(r)?;
        }
        t.tc.posts.lock().unwrap().push(post);
        Ok(())
    }
}

/// What a person is told when the retry of their failed turn found nothing to
/// do: why, and what they can do.
fn nothing_to_retry(stop_reason: &str, failing: &crate::session::Failing) -> String {
    let why = if stop_reason == "awaiting_confirm" {
        "a call of the turn is waiting for your approval: answer its card, or stop it"
    } else {
        "everything you sent has been answered, so there is no message left for me to read"
    };
    format!(
        "Nothing to retry: {why} (the turn failed with {}). Send a message to go on.",
        failing.class
    )
}
