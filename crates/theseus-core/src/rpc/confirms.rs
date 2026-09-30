//! Pending confirms (theseus-0g4): what waits for the operator, derived one
//! way (`Kernel::pending_confirms`) for every reader, and the answers.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::json;
use theseus_protocol::{notify, ConfirmRequest, Message};

use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::node::{Body, Node};
use crate::peer::Traced;
use crate::session::SessionRecord;
use crate::turn::OPERATOR;
use theseus_kernel::{Action, LimitFollowed, BUDGET_TOOL};

impl Core {
    /// A waiting action as the question the operator sees: the one place a
    /// pending confirm becomes a `ConfirmRequest` (theseus-0g4). A tool call
    /// shows its node's input and the policy's reason; None if its node is not
    /// in `nodes`.
    fn confirm_request(
        &self,
        a: &Action,
        session: &SessionRecord,
        nodes: &[(u64, Node)],
    ) -> Option<ConfirmRequest> {
        if a.tool == BUDGET_TOOL {
            return self.budget_confirm(a, session);
        }
        let (tool, input, gate) = nodes.iter().find_map(|(_, n)| match &n.body {
            Body::ToolCall {
                tool,
                input,
                correlation_id: Some(c),
                gate,
                ..
            } if *c == a.correlation_id => Some((tool, input, gate)),
            _ => None,
        })?;
        Some(ConfirmRequest {
            correlation_id: a.correlation_id.clone(),
            session_id: session.session_id.clone(),
            execution_id: a.execution_id.clone(),
            tool: tool.clone(),
            input: input.clone(),
            resource: a.resource.clone(),
            reason: gate["decision"]["reason"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            by: OPERATOR.into(),
            requested_at_ms: a.planned_at_ms,
            expires_at_ms: a.planned_at_ms + self.kernel.config().confirm_ttl_ms,
            floor: gate["decision"]["floor"].as_bool().unwrap_or(false),
            budget: None,
        })
    }

    /// An open budget question, as the confirm it is (theseus-0sg): its
    /// execution reached the spend limit and waits for the operator. The
    /// figures are the execution's now.
    fn budget_confirm(&self, q: &Action, session: &SessionRecord) -> Option<ConfirmRequest> {
        use theseus_kernel::micros_to_usd as usd;
        let e = self.kernel.execution(&q.execution_id).ok()??;
        let b = &e.budget;
        let needed = usd(b.question_needs_micros);
        Some(ConfirmRequest {
            correlation_id: q.correlation_id.clone(),
            session_id: session.session_id.clone(),
            execution_id: e.id.clone(),
            tool: BUDGET_TOOL.into(),
            input: json!({"spent_usd": usd(b.spent_micros), "limit_usd": usd(b.limit_micros), "needed_usd": needed}),
            resource: None,
            reason: format!(
                "This session has spent {} of its {} limit. Reset its spend to $0 and continue?",
                crate::narrative::dollars(b.spent_micros),
                crate::narrative::dollars(b.limit_micros)
            ),
            by: OPERATOR.into(),
            requested_at_ms: q.planned_at_ms,
            expires_at_ms: 0,
            floor: false,
            budget: Some(theseus_protocol::BudgetAsk {
                spent_usd: usd(b.spent_micros),
                limit_usd: usd(b.limit_micros),
                needed_usd: needed,
                lifetime_usd: session.cost_usd,
            }),
        })
    }

    /// A session's questions for the operator: its budget question, then its
    /// tool calls waiting for approval.
    pub fn pending_confirms(&self, session_id: &str) -> Result<Vec<ConfirmRequest>> {
        let Some(rec) = self.store.get_session::<SessionRecord>(session_id)? else {
            return Ok(vec![]);
        };
        let nodes = self.store.session_nodes(session_id)?;
        Ok(self.confirms_of(&self.kernel.pending_confirms()?, &rec, &nodes))
    }

    /// The questions in `pending` (`Kernel::pending_confirms`) that one
    /// session asks. `nodes` is its transcript.
    pub(super) fn confirms_of(
        &self,
        pending: &[Action],
        session: &SessionRecord,
        nodes: &[(u64, Node)],
    ) -> Vec<ConfirmRequest> {
        pending
            .iter()
            .filter(|a| a.session_id == session.session_id)
            .filter_map(|a| self.confirm_request(a, session, nodes))
            .collect()
    }

    /// Everything waiting for the operator (`confirm.list`, what `theseus
    /// confirm` with no id lists): the questions of every session parked on
    /// one, the most recently active session first. One scan of the open
    /// actions; a transcript is read only for a session with a tool call waiting.
    pub fn confirm_list(&self) -> Result<Vec<ConfirmRequest>> {
        let pending = self.kernel.pending_confirms()?;
        let mut out = Vec::new();
        if pending.is_empty() {
            return Ok(out);
        }
        for rec in self.sessions_by_activity()? {
            let asks: Vec<&Action> = pending
                .iter()
                .filter(|a| a.session_id == rec.session_id)
                .collect();
            let parked = rec
                .execution_id
                .as_deref()
                .and_then(|id| self.kernel.execution(id).ok().flatten())
                .is_some_and(|e| e.state == theseus_kernel::ExecState::Waiting);
            if asks.is_empty() || !parked {
                continue;
            }
            let nodes = if asks.iter().any(|a| a.tool != BUDGET_TOOL) {
                self.store.session_nodes(&rec.session_id)?
            } else {
                vec![]
            };
            out.extend(
                asks.into_iter()
                    .filter_map(|a| self.confirm_request(a, &rec, &nodes)),
            );
        }
        Ok(out)
    }

    /// Answer a confirm: bind it (approve) or decline the action, then wake the
    /// execution so the driver resumes the turn exactly where it parked.
    ///
    /// This is the one place an answer becomes a decision, for a tool call and
    /// a budget question alike, so `[approval]` is judged here (theseus-sgh):
    /// an answer that does not count is refused with the reason, ledgered as
    /// `approval.refused`, and narrated, and the question keeps waiting. `by`
    /// is who answered and through what; a bare label is no known surface.
    /// The process that answered, when the connection knows one, is recorded
    /// with the answer (theseus-6qy).
    pub fn confirm_action(
        &self,
        correlation_id: &str,
        approve: bool,
        note: Option<&str>,
        by: impl Into<Answerer>,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let who = by.into();
        let a = self
            .kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow::anyhow!("no action {correlation_id}"))?;
        let asker = self.judge_act(
            &who,
            Act::Answer {
                action: &a,
                approve,
            },
        )?;
        let by = who.label.as_str();
        if a.state != theseus_kernel::ActionState::Planned {
            anyhow::bail!(
                "action {correlation_id} is {}, not waiting for confirmation",
                a.state.as_str()
            );
        }
        let via = who.via();
        if a.tool == BUDGET_TOOL {
            return self.answer_budget(&a, approve, note, by, &via, &asker);
        }
        if approve {
            let proposal = crate::toolrun::confirm_proposal(&self.store, &a, None)?;
            self.kernel
                .bind_confirm(correlation_id, OPERATOR, &proposal)?;
        } else {
            self.kernel.decline_action(
                correlation_id,
                OPERATOR,
                note.unwrap_or("the operator declined"),
            )?;
        }
        self.store.append_ledger(&LedgerRow::new(
            "action.confirm_answered",
            Some(&a.session_id),
            None,
            json!({"correlation_id": correlation_id, "approved": approve, "note": note, "by": by, "via": via,
                   "asker": asker.json()}),
        ))?;
        narrate!(
            self.narrator,
            Approval,
            Some(&a.session_id),
            None,
            "{} {} by {by}{}.",
            a.tool,
            if approve { "approved" } else { "declined" },
            if note.is_some_and(|n| !n.trim().is_empty()) {
                ", with a note"
            } else {
                ""
            }
        );
        if self
            .kernel
            .wake(
                &a.execution_id,
                if approve { "confirmed" } else { "declined" },
            )
            .is_ok()
        {
            narrate!(
                self.narrator,
                Session,
                Some(&a.session_id),
                None,
                "Woken by the {}; the driver resumes the turn.",
                if approve { "approval" } else { "decline" }
            );
        }
        self.bus.publish(
            &a.session_id,
            &Message::Notification(theseus_protocol::Notification::new(
                notify::CONFIRM_RESOLVED,
                json!({"session_id": a.session_id, "correlation_id": correlation_id, "approved": approve, "by": by}),
            )),
            None,
        );
        self.admission.notify_waiters();
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: correlation_id.into(),
            approved: approve,
            session_id: a.session_id,
            execution_id: a.execution_id,
            resumes: true,
        })
    }

    /// The one judgment of every approval-like act (theseus-sgh): an answer
    /// to a waiting call (the spend reset among them), a "should have asked"
    /// press, and its undo.
    ///
    /// An answer and an undo first trace the process that asked (theseus-6qy):
    /// one that descends from a live Theseus job wrapper, or that cannot be
    /// traced, is refused, with or without `[approval]`. Then they take the
    /// whole `[approval]` rule. A press only makes calls ask, so any surface
    /// that can answer an approval may make one, a job's process included,
    /// and it is not traced.
    ///
    /// A refusal is ledgered as `approval.refused` (who, through what, and
    /// why) and narrated, and it is the error, a `Refusal`; one from a job's
    /// process is a security event, announced to every connection as well.
    /// Nothing else changes. What the trace found is returned, to be
    /// recorded with the act.
    pub(crate) fn judge_act(&self, who: &Answerer, act: Act<'_>) -> Result<Traced> {
        let traced = match act {
            Act::Tighten { .. } => Traced::NoProcess,
            Act::Answer { .. } | Act::Untighten { .. } => who.peer.trace(),
        };
        let verdict = match (act, traced.refusal()) {
            (_, Some(why)) => Err(Refusal {
                who: who.who(),
                via: who.via(),
                why,
            }),
            (Act::Tighten { .. }, None) => self.approval.judge_tighten(who),
            (_, None) => self.approval.judge(who),
        };
        let Err(r) = verdict else {
            return Ok(traced);
        };
        self.refused(act, who, &r, &traced)?;
        Err(r.into())
    }

    /// An act that does not count: ledgered with who, where, why, and the
    /// process that asked, and narrated. A refused answer leaves the action
    /// and its execution waiting; a refused undo leaves the tool asking. One
    /// from a Theseus job's process (theseus-6qy) is narrated as the security
    /// event it is, and announced as `approval.refused` to every connection,
    /// so the Discord binding tells the operator where approvals go.
    fn refused(&self, act: Act<'_>, who: &Answerer, r: &Refusal, traced: &Traced) -> Result<()> {
        let (session, mut data) = match act {
            Act::Answer { action: a, approve } => (
                Some(a.session_id.as_str()),
                json!({"correlation_id": a.correlation_id, "tool": a.tool, "approve": approve,
                       "who": r.who, "via": r.via, "why": r.why, "by": who.label}),
            ),
            Act::Tighten { tool } | Act::Untighten { tool } => (
                None,
                json!({"act": act.method(), "tool": tool, "who": r.who, "via": r.via,
                       "why": r.why, "by": who.label}),
            ),
        };
        if *traced != Traced::NoProcess {
            data["asker"] = traced.json();
        }
        let from_job = traced.refusal().is_some();
        if from_job {
            data["from_job"] = json!(true);
        }
        self.store.append_ledger(&LedgerRow::new(
            "approval.refused",
            session,
            None,
            data.clone(),
        ))?;
        if from_job {
            self.refused_from_job(act, r, session, data);
            return Ok(());
        }
        match act {
            Act::Answer { action: a, .. } => narrate!(
                self.narrator,
                Approval,
                session,
                None,
                "An answer to {} from {} through {} did not count: {}. It keeps waiting.",
                a.tool,
                r.who,
                r.via,
                r.why
            ),
            Act::Tighten { tool } => narrate!(
                self.narrator,
                Approval,
                None,
                None,
                "\"Should have asked\" for {tool} from {} through {} did not count: {}. It \
                 keeps its posture.",
                r.who,
                r.via,
                r.why
            ),
            Act::Untighten { tool } => narrate!(
                self.narrator,
                Approval,
                None,
                None,
                "An undo of {tool}'s tightening from {} through {} did not count: {}. It keeps \
                 asking first.",
                r.who,
                r.via,
                r.why
            ),
        }
        Ok(())
    }

    /// A refusal of a Theseus job's process, or of one that cannot be traced
    /// (theseus-6qy): a security event. The narrative says so, and every
    /// connection hears of it (`approval.refused`, with the ledger row's
    /// fields), the Discord binding among them, which tells the operator in
    /// the DM where approvals go.
    fn refused_from_job(
        &self,
        act: Act<'_>,
        r: &Refusal,
        session: Option<&str>,
        mut data: serde_json::Value,
    ) {
        let what = match act {
            Act::Answer { action: a, .. } => format!("an answer to {}", a.tool),
            Act::Untighten { tool } => format!("the undo of {tool}'s tightening"),
            Act::Tighten { tool } => format!("\"should have asked\" for {tool}"),
        };
        let then = match act {
            Act::Answer { .. } => "It keeps waiting for the operator's answer.",
            Act::Untighten { .. } => "It keeps asking first.",
            Act::Tighten { .. } => "Nothing changed.",
        };
        narrate!(
            self.narrator,
            Approval,
            session,
            None,
            "Refused {what} {} through {}: a job's process cannot answer an approval. {then}",
            r.why,
            r.via
        );
        data["act"] = json!(act.method());
        data["session_id"] = json!(session);
        self.bus
            .publish_all(&Message::Notification(theseus_protocol::Notification::new(
                notify::APPROVAL_REFUSED,
                data,
            )));
    }

    /// Answer a budget question (theseus-0sg). Approve: the execution's spend
    /// goes back to $0 and the driver makes the waiting call; the session's
    /// lifetime cost is untouched. Decline: the question closes and the
    /// session keeps waiting on its budget, which is not a hard no; its next
    /// message asks again, or the operator cancels it or starts `/new`.
    fn answer_budget(
        &self,
        q: &theseus_kernel::Action,
        approve: bool,
        note: Option<&str>,
        by: &str,
        via: &str,
        asker: &Traced,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let correlation_id = q.correlation_id.as_str();
        if approve {
            let (e, before) = self.kernel.reset_budget(correlation_id, by)?;
            narrate!(
                self.narrator,
                Approval,
                Some(&q.session_id),
                None,
                "Spend reset to $0 by {by} (it was {} of the {} limit); continuing.",
                crate::narrative::dollars(before),
                crate::narrative::dollars(e.budget.limit_micros)
            );
        } else {
            self.kernel.decline_action(
                correlation_id,
                by,
                note.unwrap_or("the operator declined the reset"),
            )?;
            narrate!(
                self.narrator,
                Approval,
                Some(&q.session_id),
                None,
                "The budget reset was declined by {by}; the session keeps waiting, and a new \
                 message asks again."
            );
        }
        self.store.append_ledger(&LedgerRow::new(
            "action.confirm_answered",
            Some(&q.session_id),
            None,
            json!({"correlation_id": correlation_id, "approved": approve, "note": note, "by": by, "via": via, "tool": q.tool,
                   "asker": asker.json()}),
        ))?;
        self.bus.publish(
            &q.session_id,
            &Message::Notification(theseus_protocol::Notification::new(
                notify::CONFIRM_RESOLVED,
                json!({"session_id": q.session_id, "correlation_id": correlation_id, "approved": approve, "by": by}),
            )),
            None,
        );
        self.admission.notify_waiters();
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: correlation_id.into(),
            approved: approve,
            session_id: q.session_id.clone(),
            execution_id: q.execution_id.clone(),
            resumes: approve,
        })
    }
}

impl Core {
    /// Open sessions follow `[kernel] spend_limit_usd` (theseus-3pj). The
    /// core runs this when the vault confirms the copy this start served
    /// from, before the gate opens, so nothing acts on an old limit once the
    /// vault has confirmed a new one. (A start whose config may act at once
    /// had the kernel's startup do it.) A failure is loud, and the gate still
    /// opens: a store that cannot write this frame cannot write a turn either.
    pub fn follow_spend_limit(&self) {
        match self.kernel.follow_spend_limit() {
            Ok(followed) => self.said_limits_followed(&followed),
            Err(e) => tracing::error!(
                error = %format!("{e:#}"),
                "open sessions could not take the configured spend limit; they keep the one they had"
            ),
        }
    }

    /// Say what following the spend limit did: one log line, a narrative
    /// line per session, and `confirm.resolved` to the clients of each
    /// session whose question a raise withdrew.
    pub(crate) fn said_limits_followed(&self, followed: &[LimitFollowed]) {
        let Some(first) = followed.first() else {
            return;
        };
        let proceed = followed.iter().filter(|f| f.proceeds).count();
        tracing::info!(
            limit_usd = theseus_kernel::micros_to_usd(first.to_micros),
            sessions = followed.len(),
            raised = followed
                .iter()
                .filter(|f| f.to_micros > f.from_micros)
                .count(),
            proceed,
            "open sessions follow the configured spend limit"
        );
        for f in followed {
            let then = if f.proceeds {
                "; the call that waited at the old limit proceeds"
            } else if f.to_micros < f.from_micros {
                "; its next call that does not fit asks"
            } else {
                ""
            };
            narrate!(
                self.narrator,
                Session,
                Some(&f.session_id),
                None,
                "Session {} follows the config's spend limit: {} before, {} now{then}.",
                crate::narrative::short(&f.session_id),
                crate::narrative::dollars(f.from_micros),
                crate::narrative::dollars(f.to_micros)
            );
            if let Some(q) = &f.withdrew {
                self.bus.publish(
                    &f.session_id,
                    &Message::Notification(theseus_protocol::Notification::new(
                        notify::CONFIRM_RESOLVED,
                        json!({"session_id": f.session_id, "correlation_id": q, "approved": false, "withdrawn": true, "by": "config"}),
                    )),
                    None,
                );
            }
        }
        if proceed > 0 {
            self.admission.notify_waiters();
        }
    }
}

/// An approval-like act, as `Core::judge_act` judges it (theseus-sgh).
#[derive(Clone, Copy)]
pub(crate) enum Act<'a> {
    /// An answer to a waiting call or a budget question.
    Answer { action: &'a Action, approve: bool },
    /// "Should have asked": the tool asks first from now on.
    Tighten { tool: &'a str },
    /// The undo of a tightening, which returns the tool to the config.
    Untighten { tool: &'a str },
}

impl Act<'_> {
    /// The method that makes the act, as a refusal row names it.
    fn method(self) -> &'static str {
        match self {
            Act::Answer { .. } => theseus_protocol::method::ACTION_CONFIRM,
            Act::Tighten { .. } => theseus_protocol::method::POLICY_TIGHTEN,
            Act::Untighten { .. } => theseus_protocol::method::POLICY_UNTIGHTEN,
        }
    }
}

/// How many questions each execution waits on, from `Kernel::pending_confirms`.
pub(super) fn waiting_by_execution(pending: &[Action]) -> BTreeMap<String, u32> {
    let mut by = BTreeMap::new();
    for a in pending {
        *by.entry(a.execution_id.clone()).or_insert(0) += 1;
    }
    by
}
