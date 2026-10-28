//! `work.answer` (theseus-ds5os): one answer verb for every question. The
//! question's id is its correlation id (a reminder's is the wake's), named
//! whole or by a unique end of at least four characters; the option is one
//! of the question's `AnswerOption` ids. The answer is routed to the handler
//! of the question's kind, as the calling surface's: a call, a budget
//! question, a layer-1 change and an extension's ack to `confirm_action_with`
//! (which judges the place rule exactly as `action.confirm` does), a
//! reminder's Done and snoozes to the wake owner's verbs. An answer from any
//! surface closes the question for all: one `confirm.resolved` carrying the
//! option, and the push's change.
//!
//! FAST: one map lookup for a whole id, one scan of the open questions for
//! an end; nothing on the turn's path.

use serde_json::json;
use theseus_kernel::{Action, BUDGET_TOOL};
use theseus_protocol::answer::{approves, reminder_options, WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::work::{answer_options, AnswerOption, QuestionKind, WorkKind, WorkRef};
use theseus_protocol::{error_code, wakes, ConfirmRequest, WakeNameParams, WakeSnoozeParams};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};

/// The fewest characters of an id that name a question.
const SHORTEST: usize = 4;

/// What a question's id names.
pub(crate) enum Target {
    /// A call, a budget question, a change, or an extension's ack, waiting.
    Action(Box<Action>),
    /// A reminder: its execution and the wake's whole id.
    Wake {
        execution_id: String,
        wake_id: String,
    },
}

/// The kind a request is: the one place a `ConfirmRequest` is judged to be a
/// budget question, an extension's ack, a layer-1 change, or a call.
pub(crate) fn kind_of(req: &ConfirmRequest) -> QuestionKind {
    if req.budget.is_some() {
        QuestionKind::Budget
    } else if req.tool == crate::ask::OWNER_ASK {
        QuestionKind::Ask
    } else if req.tool == crate::extend::ACK {
        QuestionKind::Extension
    } else if req.change.is_some() {
        QuestionKind::Change
    } else {
        QuestionKind::Approval
    }
}

/// A request with its answers worded (`options`): built wherever a request
/// is built, so every surface that draws one reads the same buttons.
pub(crate) fn offer(mut req: ConfirmRequest) -> ConfirmRequest {
    req.options = req.question(kind_of(&req), true).options;
    // A grant's offer is an answer of its own, after the plain yes
    // (theseus-drjs).
    if let Some(o) = &req.grant_offer {
        let at = req.options.len().min(1);
        req.options.insert(at, o.option());
    }
    req
}

/// The option an answer is, as `confirm.resolved` carries it.
pub(crate) fn option_id(kind: QuestionKind, approve: bool) -> &'static str {
    match (kind, approve) {
        (_, false) => "decline",
        (QuestionKind::Budget, true) => "reset",
        (QuestionKind::Change, true) => "accept",
        (QuestionKind::Extension, true) => "ack",
        (_, true) => "approve",
    }
}

/// How `option` of a question of `kind` is answered: whether it approves. A
/// question with options is answered by choosing one: it binds its confirm
/// (`approves`); which option is `answer_ask`'s.
pub(crate) fn route(kind: QuestionKind, option: &str) -> Result<bool, RpcFailure> {
    match kind {
        QuestionKind::Ask => match option.starts_with("choice:") {
            true => Ok(true),
            false => Err(refused(format!(
                "`{option}` is not a choice of a question with options: answer with its number"
            ))),
        },
        _ => Ok(approves(option)),
    }
}

/// The one name `given` is among `names`: whole, or a unique end of at least
/// four characters. An end that names two is refused, naming both.
pub(crate) fn pick<'a>(given: &str, names: &'a [String]) -> Result<&'a String, RpcFailure> {
    let s = given.trim().trim_start_matches('…');
    if let Some(one) = names.iter().find(|n| n.as_str() == s) {
        return Ok(one);
    }
    if s.chars().count() < SHORTEST {
        return Err(invalid(format!(
            "`{given}` is too short to name a question: give at least four characters of its id"
        )));
    }
    let ends: Vec<&String> = names.iter().filter(|n| n.ends_with(s)).collect();
    match ends.as_slice() {
        [] => Err(not_found(format!("no question is named `{given}`"))),
        [one] => Ok(one),
        many => Err(invalid(format!(
            "`{given}` names {} questions ({}): give more of its id",
            many.len(),
            many.iter()
                .take(5)
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn refused(why: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::REFUSED, why.to_string())
}

fn invalid(why: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, why.to_string())
}

fn not_found(why: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::NOT_FOUND, why.to_string())
}

/// The ids of `options`, for a refusal: `approve, decline`.
fn ids(options: &[AnswerOption]) -> String {
    options
        .iter()
        .map(|o| o.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Core {
    /// Whether `who` may answer the owner's questions, and if not, why and
    /// where to answer instead: the place rule, as `action.confirm` judges
    /// it (`Core::judge_act`), asked without making an act.
    pub(crate) fn may_answer(&self, who: &Answerer) -> Result<(), String> {
        crate::places::owner_in_private(who, &self.runner.place_rule, &self.cfg).map_err(|why| {
            format!("{why}; answer from the CLI (`theseus answer`), the web UI, or a private place")
        })
    }

    /// `confirm.list` for one connection: every open question, each saying
    /// whether this connection may answer it (the place rule), so a surface
    /// draws no button whose answer would be refused.
    pub(super) fn confirm_list_for(
        &self,
        p: theseus_protocol::answer::ConfirmListParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::ConfirmListResult, RpcFailure> {
        let who = conn.answerer(None, p.discord);
        let may = self.may_answer(&who).is_ok();
        let mut confirms = self.confirm_list()?;
        for c in &mut confirms {
            // An agent draws no button for what spends money or changes AWS (theseus-5rvss).
            let agent_barred = who.agent.is_some() && crate::approval::owners_alone(&c.tool);
            c.may_answer = Some(may && !agent_barred);
        }
        Ok(theseus_protocol::ConfirmListResult { confirms })
    }

    /// The kind of question an action is.
    pub(crate) fn kind_of_action(&self, a: &Action) -> QuestionKind {
        if a.tool == BUDGET_TOOL {
            return QuestionKind::Budget;
        }
        if a.tool == crate::ask::OWNER_ASK {
            return QuestionKind::Ask;
        }
        if a.tool == crate::extend::ACK {
            return QuestionKind::Extension;
        }
        let is_change = a.proposal.as_ref().is_some_and(|p| {
            crate::task_graph::tools::change_of(&self.store, &p.tool, &p.args).is_some()
        });
        match is_change {
            true => QuestionKind::Change,
            false => QuestionKind::Approval,
        }
    }

    /// The question `given` names: a whole id with one lookup, or a unique
    /// end of at least four characters among the open questions and pending
    /// wakes. An end that names two is refused, naming both.
    pub(crate) fn resolve_question(&self, given: &str) -> Result<Target, RpcFailure> {
        let s = given.trim().trim_start_matches('…');
        if s.starts_with("wak_") {
            return match self.fired_or_pending(s)? {
                Some((execution_id, wake_id)) => Ok(Target::Wake {
                    execution_id,
                    wake_id,
                }),
                None => Err(not_found(format!("no reminder is named `{given}`"))),
            };
        }
        if s.contains('_') {
            return match self.kernel.action(s)? {
                Some(a) => Ok(Target::Action(Box::new(a))),
                None => self
                    .question_in(given)?
                    .map(Target::Action)
                    .ok_or_else(|| not_found(format!("no question is named `{given}`"))),
            };
        }
        // An end: among the open questions and the pending wakes.
        let actions = self.kernel.pending_confirms()?;
        let wakes = self.kernel.pending_wakes()?;
        let names: Vec<String> = actions
            .iter()
            .map(|a| a.correlation_id.clone())
            .chain(wakes.iter().map(|(_, w)| w.id.clone()))
            .collect();
        let name = match pick(given, &names) {
            Ok(name) => name,
            Err(e) if e.code == error_code::NOT_FOUND => {
                return self.question_in(given)?.map(Target::Action).ok_or(e);
            }
            Err(e) => return Err(e),
        };
        if let Some(a) = actions.into_iter().find(|a| &a.correlation_id == name) {
            return Ok(Target::Action(Box::new(a)));
        }
        let (e, w) = wakes
            .into_iter()
            .find(|(_, w)| &w.id == name)
            .expect("picked from them");
        Ok(Target::Wake {
            execution_id: e.id,
            wake_id: w.id,
        })
    }

    /// The one open question waiting in the session or the execution `given`
    /// names, whole or by a unique end of at least four characters: what
    /// `theseus confirm` took beside a question's own id.
    fn question_in(&self, given: &str) -> Result<Option<Box<Action>>, RpcFailure> {
        let s = given.trim().trim_start_matches('…');
        if s.chars().count() < SHORTEST {
            return Ok(None);
        }
        let mut held: Vec<Action> = self
            .kernel
            .pending_confirms()?
            .into_iter()
            .filter(|a| {
                [&a.session_id, &a.execution_id]
                    .iter()
                    .any(|id| id.ends_with(s))
            })
            .collect();
        match held.len() {
            0 | 1 => Ok(held.pop().map(Box::new)),
            n => Err(invalid(format!(
                "`{given}` names {n} questions ({}): give more of its id",
                held.iter()
                    .take(5)
                    .map(|a| a.correlation_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// A reminder by its whole id: pending, or fired and kept for a snooze.
    fn fired_or_pending(&self, id: &str) -> Result<Option<(String, String)>, RpcFailure> {
        let pending = self.kernel.pending_wakes()?;
        if let Some((e, w)) = pending.into_iter().find(|(_, w)| w.id == id) {
            return Ok(Some((e.id, w.id)));
        }
        Ok(self.fired_wake(id)?.map(|(e, w)| (e, w.id)))
    }

    /// `work.answer`.
    pub(super) fn work_answer(
        &self,
        p: WorkAnswerParams,
        conn: Conn<'_>,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        match self.resolve_question(&p.question_id)? {
            // A routine's failure or block notice is the routine's wake
            // (theseus-52tut): its own answers.
            Target::Wake {
                execution_id,
                wake_id,
            } => match self
                .runner
                .tools
                .routines
                .by_execution(&self.store, &execution_id)
            {
                Some(spec) => self.answer_routine(&p, who, &spec),
                None => self.answer_reminder(&p, &who, conn, &execution_id, &wake_id),
            },
            Target::Action(a) if p.option == theseus_protocol::answer::ASK_AGAIN => {
                self.ask_again(&p, who, &a)
            }
            Target::Action(a) => self.answer_action(&p, who, conn, &a),
        }
    }

    /// A call, a budget question, a change or an extension's ack.
    fn answer_action(
        &self,
        p: &WorkAnswerParams,
        who: Answerer,
        conn: Conn<'_>,
        a: &Action,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        let kind = self.kind_of_action(a);
        if kind == QuestionKind::Ask {
            return self.answer_ask(p, who, conn, a);
        }
        let options = answer_options(&self.bare_question(a, kind));
        // Approve and take the question's grant (theseus-drjs): the offer
        // the daemon makes now, never one the answer names.
        if p.option == theseus_protocol::grants::APPROVE_GRANT && kind == QuestionKind::Approval {
            return self.answer_granting(p, who, conn, a);
        }
        let Some(chosen) = options.iter().find(|o| o.id == p.option) else {
            return Err(invalid(format!(
                "`{}` is not an answer to this question; the answers are: {}",
                p.option,
                ids(&options)
            )));
        };
        let approve = route(kind, &chosen.id)?;
        if p.watch {
            self.bus.watch(&a.session_id, conn.client, conn.tx.clone());
        }
        let done = self
            .confirm_action_with(
                &a.correlation_id,
                approve,
                p.note.as_deref(),
                who,
                Default::default(),
            )
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: r.to_string(),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        let resumes = done
            .resumes
            .then(|| self.work_ref_of(&done.execution_id, &done.session_id));
        // A declined budget question resumes nothing: the session waits on it.
        let detail = (kind == QuestionKind::Budget && !approve).then(|| {
            format!(
                "{} keeps waiting on its budget; a new message asks again",
                done.session_id
            )
        });
        Ok(WorkAnswerResult {
            question_id: a.correlation_id.clone(),
            option: chosen.id.clone(),
            resumes,
            detail,
        })
    }

    /// `approve_grant`: approve the call and take the grant the daemon
    /// offers for it now (theseus-drjs), through `confirm_granting`, which
    /// judges the place rule as any answer.
    fn answer_granting(
        &self,
        p: &WorkAnswerParams,
        who: Answerer,
        conn: Conn<'_>,
        a: &Action,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        let offered = self
            .pending_confirms(&a.session_id)?
            .into_iter()
            .find(|q| q.correlation_id == a.correlation_id)
            .and_then(|q| q.grant_offer)
            .ok_or_else(|| invalid("this question offers no grant: approve it without one"))?;
        if p.watch {
            self.bus.watch(&a.session_id, conn.client, conn.tx.clone());
        }
        let answer = theseus_protocol::grants::GrantAnswer {
            pattern: offered.pattern,
        };
        let done = self
            .confirm_granting(&a.correlation_id, p.note.as_deref(), who, &answer)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: r.to_string(),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        let resumes = done
            .resumes
            .then(|| self.work_ref_of(&done.execution_id, &done.session_id));
        Ok(WorkAnswerResult {
            question_id: a.correlation_id.clone(),
            option: theseus_protocol::grants::APPROVE_GRANT.into(),
            resumes,
            detail: None,
        })
    }

    /// A question with options (`owner_ask`, theseus-j7cje): `option` is a
    /// number from 1, `choice:<i>` from 0, or `other` with the owner's words
    /// in `note`. The confirm binds in the answer's own frame, with the
    /// choice's record, and the call reads it back as its result.
    fn answer_ask(
        &self,
        p: &WorkAnswerParams,
        who: Answerer,
        conn: Conn<'_>,
        a: &Action,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        use theseus_protocol::work::ask::{choice_id, choice_of, Choice};
        let ask = crate::ask::input_of(a)
            .ok_or_else(|| invalid("this question's options could not be read"))?;
        let n = ask.options.len();
        let note = p.note.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let (option, text) = match choice_of(&p.option, n) {
            Some(Choice::Pick(i)) => (choice_id(i), note),
            Some(Choice::Other) => match note {
                Some(t) => ("choice:other".to_string(), Some(t)),
                None => {
                    return Err(invalid(
                        "`other` is your own words: add them as the note (`--note <words>`)",
                    ))
                }
            },
            None => {
                return Err(invalid(format!(
                    "`{}` is not an answer to this question; the answers are: 1 to {n}, or \
                     other with a note",
                    p.option
                )))
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
        .map_err(RpcFailure::invalid)?;
        if p.watch {
            self.bus.watch(&a.session_id, conn.client, conn.tx.clone());
        }
        let done = self
            .confirm_action_with(
                &a.correlation_id,
                true,
                text,
                who,
                super::confirms::Rides {
                    extra: &[],
                    ask: Some((&option, record)),
                },
            )
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: r.to_string(),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        let resumes = done
            .resumes
            .then(|| self.work_ref_of(&done.execution_id, &done.session_id));
        Ok(WorkAnswerResult {
            question_id: a.correlation_id.clone(),
            option,
            resumes,
            detail: None,
        })
    }

    /// A question's shell, for its options: its kind and floor are what they
    /// ask of `answer_options`.
    fn bare_question(&self, a: &Action, kind: QuestionKind) -> theseus_protocol::work::Question {
        theseus_protocol::work::Question {
            id: a.correlation_id.clone(),
            kind,
            asked_by: self.work_ref_of(&a.execution_id, &a.session_id),
            prompt: a.tool.clone(),
            detail: None,
            input: None,
            options: Vec::new(),
            on_expiry: None,
            expires_at_ms: None,
            floor: false,
            may_answer: true,
            tool: Some(a.tool.clone()),
            asked_at_ms: a.planned_at_ms,
            choices: crate::ask::input_of(a)
                .map(|i| i.choices())
                .unwrap_or_default(),
        }
    }

    /// The work an execution is, as an answer names what it resumes.
    fn work_ref_of(&self, execution_id: &str, session_id: &str) -> WorkRef {
        let task = self
            .store
            .get_session::<crate::session::SessionRecord>(session_id)
            .ok()
            .flatten()
            .and_then(|r| r.task);
        WorkRef {
            id: execution_id.into(),
            kind: match task {
                Some(_) => WorkKind::Task,
                None => WorkKind::Conversation,
            },
            session_id: Some(session_id.into()),
            title: String::new(),
        }
    }

    /// A reminder's Done and its three snoozes: the wake owner's verbs. They
    /// are anyone's (`wake_owner.rs`), but a surface that is never an
    /// approval surface (an MCP client, a connection no listener named) does
    /// not answer for the owner.
    fn answer_reminder(
        &self,
        p: &WorkAnswerParams,
        who: &Answerer,
        conn: Conn<'_>,
        execution_id: &str,
        wake_id: &str,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        if let Some(why) = who.unknown() {
            return Err(refused(why));
        }
        let options = reminder_options();
        if !options.iter().any(|o| o.id == p.option) {
            return Err(invalid(format!(
                "`{}` is not an answer to a reminder; the answers are: {}",
                p.option,
                ids(&options)
            )));
        }
        let author = p.author.clone();
        if p.option == "done" {
            self.wake_done(
                WakeNameParams {
                    wake: wake_id.into(),
                    author,
                },
                conn,
            )?;
        } else {
            let when = wakes::reminder_answers()
                .into_iter()
                .find(|a| a.id == p.option)
                .and_then(|a| a.when);
            self.wake_snooze(
                WakeSnoozeParams {
                    wake: wake_id.into(),
                    when,
                    fired: false,
                    author,
                },
                conn,
            )?;
        }
        let _ = execution_id;
        Ok(WorkAnswerResult {
            question_id: wake_id.into(),
            option: p.option.clone(),
            resumes: None,
            detail: None,
        })
    }
}

/// The pure parts, for the tests of the answer (theseus-ds5os).
#[cfg(test)]
pub(crate) mod answer_tests {
    pub(crate) use super::{pick, route};
}
