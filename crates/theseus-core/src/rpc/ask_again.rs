//! "Ask again" (theseus-27p6w): the button on an expired card. A question
//! that nobody answered in time closed as the model's "Not run: nobody
//! answered", and the owner who comes back later has no card to press. The
//! settled card offers one (the outbox's settle says `again` while the
//! question's execution can still take a turn), and its press is `work.answer`
//! with the option `ask_again`: judged as an answer is (the owner, from a
//! private place), it sets a wake due now on the session's execution whose
//! note tells the model what the owner asked for. The wake is the one
//! mechanism a session already has for a turn that comes from outside, so a
//! live conversation or a waiting task takes it as its next turn and plans
//! the call again; the new question is prompted (`Outbox::prompted`), so its
//! card posts at once, pinging, whatever the wait and the window say.

use theseus_kernel::{Action, NewWake, Purpose, WakeRun};
use theseus_protocol::answer::{WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::error_code;

use super::confirms::{Act, EXPIRY};
use super::server::RpcFailure;
use super::Core;
use crate::approval::{Answerer, Refusal};

/// The note's text: one line, with what the question wanted as its card said it.
fn note_of(line: &str) -> String {
    format!("The owner asked again for: {line}")
}

impl Core {
    /// Whether a closed question closed by expiry: an `owner_ask`'s own record
    /// says so, any other question's resolution names the expiry.
    fn closed_by_expiry(&self, a: &Action) -> bool {
        if a.tool == crate::ask::OWNER_ASK {
            let key = format!("{}{}", crate::ask::ANSWER_PREFIX, a.correlation_id);
            return self
                .store
                .get_meta::<crate::ask::AskAnswer>(&key)
                .ok()
                .flatten()
                .is_some_and(|r| r.expired);
        }
        a.resolution
            .as_deref()
            .is_some_and(|r| r.starts_with(&format!("declined by {EXPIRY}")))
    }

    /// `work.answer` with `ask_again` on a question that expired.
    pub(super) fn ask_again(
        &self,
        p: &WorkAnswerParams,
        who: Answerer,
        a: &Action,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        self.judge_act(
            &who,
            Act::Answer {
                action: a,
                approve: true,
            },
        )
        .map_err(|e| match e.downcast::<Refusal>() {
            Ok(r) => RpcFailure {
                code: error_code::REFUSED,
                message: r.to_string(),
                data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
            },
            Err(e) => RpcFailure::invalid(e),
        })?;
        if !self.closed_by_expiry(a) {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "only a question that expired can be asked again",
            ));
        }
        let line = self.what_of(a).unwrap_or_else(|| format!("`{}`", a.tool));
        let new = NewWake {
            due_at_ms: self.kernel.now_ms(),
            note: note_of(&line),
            target: self.outbox.target(&a.session_id),
            repeat: None,
            purpose: Purpose::Check,
            run: WakeRun::Turn,
        };
        let id = theseus_kernel::wake_id(&format!("{}_again", a.correlation_id));
        let set = self
            .kernel
            .create_wake(&a.execution_id, Some(id), new, &who.label)
            .map_err(|e| {
                RpcFailure::new(
                    error_code::REFUSED,
                    format!("its session can take no more turns: {e:#}"),
                )
            })?;
        self.outbox.prompted.add(&a.session_id);
        Ok(WorkAnswerResult {
            question_id: a.correlation_id.clone(),
            option: p.option.clone(),
            resumes: None,
            detail: Some(match set.set {
                true => "asked again".into(),
                false => "already asked again".into(),
            }),
        })
    }
}
