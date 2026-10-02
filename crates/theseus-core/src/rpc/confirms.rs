//! Pending confirms (theseus-0g4): what waits for the operator, derived one
//! way (`Kernel::pending_confirms`) for every reader, and the answers.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::json;
use theseus_protocol::{
    ApprovalRefused, ConfirmRequest, ConfirmResolved, Event, GateDecision, Message, PendingConfirm,
};

use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::node::{Body, Node};
use crate::outbox::Closed;
use crate::peer::Traced;
use crate::session::SessionRecord;
use crate::turn::OPERATOR;
use theseus_kernel::{Action, LimitFollowed, BUDGET_TOOL};
use theseus_store::Store as _;

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
        let decision = gate
            .as_ref()
            .and_then(|g| g.decision.clone())
            .unwrap_or_default();
        Some(ConfirmRequest {
            correlation_id: a.correlation_id.clone(),
            session_id: session.session_id.clone(),
            execution_id: a.execution_id.clone(),
            tool: tool.clone(),
            input: input.clone(),
            resource: a.resource.clone(),
            reason: decision.reason,
            by: OPERATOR.into(),
            requested_at_ms: a.planned_at_ms,
            expires_at_ms: a.planned_at_ms + self.kernel.config().confirm_ttl_ms,
            floor: decision.floor,
            budget: None,
            task: crate::task::task_ref(session),
            // The call waits because its session read external text
            // (theseus-9bp): its card offers to trust the session again.
            external_text: decision.external,
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
            reason: crate::turn::budget_question(
                &match &session.task {
                    Some(_) => format!("Task {}", crate::task::short(&session.session_id)),
                    None => "This session".to_string(),
                },
                b.spent_micros,
                b.limit_micros,
                b.question_needs_micros,
                &q.proposal
                    .as_ref()
                    .map(|p| p.args["call"].clone())
                    .unwrap_or_default(),
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
            task: crate::task::task_ref(session),
            external_text: None,
        })
    }

    /// A card's question as the operator sees it, from its action and, for a
    /// tool call, the `ToolCall` node its card names (theseus-q4v): one read
    /// by key, not a transcript.
    pub fn question_request(
        &self,
        q: &Action,
        node_id: Option<&str>,
    ) -> Result<Option<ConfirmRequest>> {
        let Some(rec) = self.store.get_session::<SessionRecord>(&q.session_id)? else {
            return Ok(None);
        };
        if q.tool == BUDGET_TOOL {
            return Ok(self.budget_confirm(q, &rec));
        }
        let Some(id) = node_id else {
            return Ok(None);
        };
        let node = self
            .store
            .inner()
            .latest_by_key(theseus_store::kinds::NODE, id)?
            .map(|r| r.decode::<Node>())
            .transpose()?;
        Ok(node.and_then(|n| self.confirm_request(q, &rec, &[(0, n)])))
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

    /// Each execution's questions in brief (theseus-in3), from `pending`
    /// (`Kernel::pending_confirms`, a budget question first). A tool call's
    /// reason and floor are the gate's, on its node, so each session with one
    /// waiting has its transcript read once; `known` is one already read.
    pub(crate) fn pending_by_execution(
        &self,
        pending: &[Action],
        known: Option<(&str, &[(u64, Node)])>,
    ) -> BTreeMap<String, Vec<PendingConfirm>> {
        let ttl = self.kernel.config().confirm_ttl_ms;
        let mut read: BTreeMap<String, Vec<(u64, Node)>> = BTreeMap::new();
        let mut out: BTreeMap<String, Vec<PendingConfirm>> = BTreeMap::new();
        for a in pending {
            let decision = match (a.tool == BUDGET_TOOL, known) {
                (true, _) => None,
                (false, Some((sid, nodes))) if sid == a.session_id => {
                    decision_of(nodes, &a.correlation_id)
                }
                (false, _) => {
                    let nodes = read.entry(a.session_id.clone()).or_insert_with(|| {
                        self.store.session_nodes(&a.session_id).unwrap_or_default()
                    });
                    decision_of(nodes, &a.correlation_id)
                }
            };
            out.entry(a.execution_id.clone())
                .or_default()
                .push(crate::push::pending_of(a, decision.as_ref(), ttl));
        }
        out
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
        self.confirm_action_with(correlation_id, approve, note, by, false)
    }

    /// `confirm_action`, and with `trust`, trust the call's session again as
    /// well (theseus-9bp): its hold on external text is cleared before the
    /// approval wakes the execution, so the calls after this one run at their
    /// postures. The answer's judgment covers the trust, which only goes with
    /// an approval of a tool call.
    pub fn confirm_action_with(
        &self,
        correlation_id: &str,
        approve: bool,
        note: Option<&str>,
        by: impl Into<Answerer>,
        trust: bool,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let who = by.into();
        let a = self
            .kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow::anyhow!("no action {correlation_id}"))?;
        if trust && (!approve || a.tool == BUDGET_TOOL) {
            anyhow::bail!(
                "trust goes with an approval of a tool call: approve {correlation_id} to trust \
                 its session, or use `policy.trust`"
            );
        }
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
            if trust {
                self.clear_hold(
                    &a.session_id,
                    &who,
                    theseus_protocol::method::ACTION_CONFIRM,
                    Some(correlation_id),
                    &asker,
                )?;
            }
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
                   "asker": asker.json(), "trust": trust}),
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
            &Message::from(Event::ConfirmResolved(ConfirmResolved {
                session_id: a.session_id.clone(),
                correlation_id: correlation_id.into(),
                approved: approve,
                by: Some(by.into()),
                trust: Some(trust),
                ..Default::default()
            })),
            None,
        );
        self.card_closed(
            correlation_id,
            Closed {
                note: trust.then(|| "and trusted the session again".to_string()),
                ..Closed::new(if approve { "approved" } else { "declined" }, Some(by))
            },
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

    /// A question closed: its card's settle goes to the outbox (theseus-q4v).
    pub(crate) fn card_closed(&self, question: &str, how: Closed) {
        if let Err(e) = self.outbox.closed(question, how) {
            tracing::warn!(error = %format!("{e:#}"), question, "the card's settle was not written");
        }
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
            Act::Answer { .. } | Act::Untighten { .. } | Act::Trust { .. } => who.peer.trace(),
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
            Act::Trust { session } => (
                Some(session),
                json!({"act": act.method(), "session_id": session, "who": r.who, "via": r.via,
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
            self.refused_from_job(act, who, r, session, traced);
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
            Act::Trust { .. } => narrate!(
                self.narrator,
                Approval,
                session,
                None,
                "Trusting this session again, from {} through {}, did not count: {}. Its calls \
                 that act keep waiting.",
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
        who: &Answerer,
        r: &Refusal,
        session: Option<&str>,
        traced: &Traced,
    ) {
        let what = match act {
            Act::Answer { action: a, .. } => format!("an answer to {}", a.tool),
            Act::Untighten { tool } => format!("the undo of {tool}'s tightening"),
            Act::Tighten { tool } => format!("\"should have asked\" for {tool}"),
            Act::Trust { session } => format!(
                "trusting session {} again",
                crate::narrative::short(session)
            ),
        };
        let then = match act {
            Act::Answer { .. } => "It keeps waiting for the operator's answer.",
            Act::Untighten { .. } => "It keeps asking first.",
            Act::Tighten { .. } => "Nothing changed.",
            Act::Trust { .. } => "It still holds external text, and its calls that act wait.",
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
        let (correlation_id, tool, approve) = match act {
            Act::Answer { action: a, approve } => (
                Some(a.correlation_id.clone()),
                Some(a.tool.clone()),
                Some(approve),
            ),
            Act::Tighten { tool } | Act::Untighten { tool } => (None, Some(tool.into()), None),
            Act::Trust { .. } => (None, None, None),
        };
        let refusal = ApprovalRefused {
            act: act.method().into(),
            session_id: session.map(str::to_string),
            correlation_id,
            tool,
            approve,
            who: r.who.clone(),
            via: r.via.clone(),
            why: r.why.clone(),
            by: who.label.clone(),
            asker: traced.asker(),
            from_job: true,
        };
        // A security event the operator must see, where approvals go, whether
        // or not the binding is there now (theseus-q4v).
        if let Err(e) = self
            .outbox
            .to_operator(session, json!({"kind": "refusal", "params": refusal}))
        {
            tracing::warn!(error = %format!("{e:#}"), "the refusal's notice was not written");
        }
        self.bus
            .publish_all(&Message::from(Event::ApprovalRefused(refusal)));
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
            &Message::from(Event::ConfirmResolved(ConfirmResolved {
                session_id: q.session_id.clone(),
                correlation_id: correlation_id.into(),
                approved: approve,
                by: Some(by.into()),
                ..Default::default()
            })),
            None,
        );
        self.card_closed(
            correlation_id,
            Closed::new(if approve { "approved" } else { "declined" }, Some(by)),
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
                    &Message::from(Event::ConfirmResolved(ConfirmResolved {
                        session_id: f.session_id.clone(),
                        correlation_id: q.clone(),
                        by: Some("config".into()),
                        withdrawn: true,
                        ..Default::default()
                    })),
                    None,
                );
                // S1's stale card (theseus-3pj): the raise closed it, and the
                // card says so, whenever its binding is back.
                self.card_closed(
                    q,
                    Closed {
                        how: "withdrawn".into(),
                        by: None,
                        note: Some(format!(
                            "the spend limit was raised from {} to {}",
                            crate::narrative::dollars(f.from_micros),
                            crate::narrative::dollars(f.to_micros)
                        )),
                    },
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
    /// Trusting a session again (theseus-9bp): it no longer holds external
    /// text, so its calls that act return to their postures.
    Trust { session: &'a str },
}

impl Act<'_> {
    /// The method that makes the act, as a refusal row names it.
    fn method(self) -> &'static str {
        match self {
            Act::Answer { .. } => theseus_protocol::method::ACTION_CONFIRM,
            Act::Tighten { .. } => theseus_protocol::method::POLICY_TIGHTEN,
            Act::Untighten { .. } => theseus_protocol::method::POLICY_UNTIGHTEN,
            Act::Trust { .. } => theseus_protocol::method::POLICY_TRUST,
        }
    }
}

/// The gate's decision on the tool call `correlation_id` names, from its
/// node in `nodes`, a session's transcript.
fn decision_of(nodes: &[(u64, Node)], correlation_id: &str) -> Option<GateDecision> {
    nodes.iter().rev().find_map(|(_, n)| match &n.body {
        Body::ToolCall {
            correlation_id: Some(c),
            gate,
            ..
        } if c == correlation_id => gate.as_ref().and_then(|g| g.decision.clone()),
        _ => None,
    })
}
