//! The core's side of `chat.answer_question` (chat-acts-are-tools, S43-1):
//! the one question the model named, answered through
//! [`Core::confirm_action_with`] — **the one place an answer becomes a
//! decision** — so the place rule, the peer rule and the ledger row are the
//! button's and the CLI's, to the letter. Nothing about what an answer MEANS
//! is widened here: only which inputs reach that call.
//!
//! This is what replaced `rpc/answer_in_words.rs`. That module read the owner's
//! words (`says_yes`, a list of phrases and a list of negations) and could
//! answer only a bare *yes*; the tool reads nothing, is offered only where a
//! question is actually waiting, and carries whichever answer he gave.

use theseus_protocol::work::ask::{choice_id, choice_of, Choice};
use theseus_protocol::ConfirmRequest;

use super::Core;
use crate::approval::Answerer;
use crate::yes_fact;

/// How the tool's `answer` reads for a plain question: yes, or no.
fn approves(answer: &str) -> Option<bool> {
    match answer.trim().to_lowercase().as_str() {
        "yes" | "y" | "approve" | "approved" | "ok" | "okay" => Some(true),
        "no" | "n" | "decline" | "declined" | "reject" => Some(false),
        _ => None,
    }
}

impl Core {
    /// Stage what this `turn.submit` is, for the chat acts
    /// (chat-acts-are-tools, S43-1). Called for every message, and it reads
    /// **none of the message's words**: who spoke (the `Answerer` every
    /// approval-like act is judged on), what they said (which only
    /// `allow_said.v1` ever sees, as a judge's question), and whether
    /// `Core::owner_input` counts it as the owner's own typed input from a
    /// private place.
    ///
    /// A job's turn (`opened_from`), a prompt's, an MCP client's and an
    /// unnamed surface's are staged with `owner_input` false, so the allow
    /// tool refuses in them; they are staged rather than skipped so that a
    /// turn one of them started still CLAIMS the slot, and a message of the
    /// owner's cannot be carried into a turn it did not start.
    pub(super) fn hear_for_chat_acts(
        &self,
        p: &theseus_protocol::TurnSubmitParams,
        conn: super::server::Conn<'_>,
        session: &str,
    ) {
        let who = conn.answerer(
            p.author.clone(),
            p.speaking.as_ref().map(|s| s.discord.clone()),
        );
        let owner_input = p.attachments.is_empty()
            && !matches!(
                conn.surface,
                crate::approval::Surface::Mcp | crate::approval::Surface::Unnamed
            )
            && self.owner_input(p, &who);
        self.runner.tools.chat_acts.heard.stage(
            session,
            crate::chat_acts::Said {
                text: p.input.clone(),
                who,
                owner_input,
            },
        );
    }

    /// An approval, a change or an extension's ack: `yes` or `no`.
    pub(crate) fn answer_plain_act(
        &self,
        q: &ConfirmRequest,
        answer: &str,
        note: Option<&str>,
        who: Answerer,
    ) -> Result<String, String> {
        let approve = approves(answer).ok_or_else(|| {
            format!("`{answer}` is not an answer to this question: answer it with `yes` or `no`")
        })?;
        self.confirm_action(&q.correlation_id, approve, note, who)
            .map_err(|e| format!("the answer was not taken: {e:#}"))?;
        Ok(match approve {
            true => "Approved; it runs on its own, so do not ask for it again and do not do its \
                     work by hand"
                .into(),
            false => "Declined; it does not run".to_string(),
        })
    }

    /// A question with options (`owner_ask`): the option's number from 1, or
    /// `other` with the operator's own words.
    pub(crate) fn answer_ask_act(
        &self,
        q: &ConfirmRequest,
        answer: &str,
        note: Option<&str>,
        who: Answerer,
    ) -> Result<String, String> {
        let a = self
            .kernel
            .action(&q.correlation_id)
            .map_err(|e| format!("this question could not be read: {e:#}"))?
            .ok_or("this question is no longer waiting")?;
        let ask = crate::ask::input_of(&a).ok_or("this question's options could not be read")?;
        let n = ask.options.len();
        let (option, text) = match choice_of(answer, n) {
            Some(Choice::Pick(i)) => (choice_id(i), note),
            Some(Choice::Other) => match note {
                Some(t) => ("choice:other".to_string(), Some(t)),
                None => {
                    return Err("`other` is the operator's own words: put them in `note`".into())
                }
            },
            None => {
                return Err(format!(
                    "`{answer}` is not an answer to this question; the answers are 1 to {n}, or \
                     `other` with a note"
                ))
            }
        };
        let record = crate::ask::AskAnswer {
            option: option.clone(),
            note: text.map(str::to_string),
            by: who.label.clone(),
            at_ms: self.kernel.now_ms(),
            expired: false,
        }
        .record(&a.correlation_id)
        .map_err(|e| format!("the answer could not be recorded: {e:#}"))?;
        self.confirm_action_with(
            &a.correlation_id,
            true,
            text,
            who,
            super::confirms::Rides {
                extra: &[],
                ask: Some((&option, record)),
            },
        )
        .map_err(|e| format!("the answer was not taken: {e:#}"))?;
        Ok(format!("Answered with {option}"))
    }

    /// What the harness did with the operator's message, staged for the rest
    /// of this turn (`yes_fact.rs`, `[standing] yes_fact`, off by default):
    /// what was answered, and — the half that matters — what was NOT, so the
    /// turn cannot read a silence as either. One question answered leaves
    /// every other one here waiting, and says so.
    pub(crate) fn stage_answer_fact(
        &self,
        session: &str,
        name: &str,
        waiting: &[ConfirmRequest],
        answered: &ConfirmRequest,
    ) {
        if !self.cfg.standing.yes_fact {
            return;
        }
        let rest: Vec<String> = waiting
            .iter()
            .filter(|q| q.correlation_id != answered.correlation_id)
            .map(crate::chat_acts::named_question)
            .collect();
        self.runner.yes_facts.stage(
            session,
            yes_fact::render(&yes_fact::Decided {
                why: "and the model called the answer tool for the one it named".into(),
                approved: Some(name.to_string()),
                waiting: rest,
                next: "It is answered and runs on its own: do not ask for it again, and do not \
                       do its work by hand."
                    .into(),
            }),
        );
    }
}
