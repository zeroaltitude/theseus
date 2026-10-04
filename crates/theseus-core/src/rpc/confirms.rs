//! Pending confirms (theseus-0g4): what waits for the operator, derived one
//! way (`Kernel::pending_confirms`) for every reader, and the answers.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::json;
use theseus_protocol::{ConfirmRequest, GateDecision, PendingConfirm};

use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::node::{Body, Node, ResultStatus};
use crate::outbox::Closed;
use crate::session::SessionRecord;
use crate::turn::OPERATOR;
use theseus_kernel::{Action, LimitFollowed, BUDGET_TOOL};
use theseus_store::Store as _;

/// Who declines a question nobody answered in time (theseus-830), as its
/// `action.declined` row and its resolution name it.
pub(crate) const EXPIRY: &str = "expiry";

/// The resolution an expiry writes, before its reason (`Kernel::
/// decline_action`'s `declined by <who>: <reason>`).
const EXPIRED: &str = "declined by expiry: ";

/// What the call of a question nobody answered in time tells the model
/// (theseus-830): an expiry is no one's decline. None for any other call.
pub(crate) fn expired_answer(a: &Action) -> Option<(ResultStatus, String)> {
    let why = a.resolution.as_deref()?.strip_prefix(EXPIRED)?;
    Some((
        ResultStatus::Declined,
        format!("Not run: {why}, so the request expired."),
    ))
}

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
        if a.tool == crate::extend::ACK {
            let ttl = self.kernel.config().confirm_ttl_ms;
            return Some(crate::extend::confirm_request(a, session, ttl));
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
                crate::turn::Kept {
                    reserved: b.reserved_micros,
                    unknown: b.held_unknown_micros,
                },
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
        if q.tool == crate::extend::ACK {
            return Ok(self.confirm_request(q, &rec, &[]));
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
            let nodes = if asks
                .iter()
                .any(|a| a.tool != BUDGET_TOOL && a.tool != crate::extend::ACK)
            {
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
    /// a budget question alike, so the place rule is judged here
    /// (theseus-zmgb): an answer that does not count is refused with the
    /// reason, ledgered as `approval.refused`, and narrated, and the question
    /// keeps waiting. `by`
    /// is who answered and through what; a bare label is no known surface.
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
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
        self.judge_act(
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
            return self.answer_budget(&a, approve, note, by, &via);
        }
        // A proposed extension's ack (M7 43a): nothing loads, and nothing
        // wakes.
        if a.tool == crate::extend::ACK {
            let answer = match approve {
                true => crate::extend::answer::Answer::Ack,
                false => crate::extend::answer::Answer::Decline(note),
            };
            let done = self.answer_extension(&a, answer, by, &via)?;
            self.admission.notify_waiters();
            return Ok(done);
        }
        // The answer is one frame, a kernel transaction (theseus-jj9f): the
        // bind or the decline, an approval's trust, the answer's row, and the
        // wake. So no surface sees the execution waiting on a question it no
        // longer has, and the hold is cleared in the frame that wakes it. A
        // trust takes the session's lock first: the session, then the
        // execution.
        let fact = fact::answer::CallAnswered {
            action: &a,
            approve,
            note,
            by,
            via: &via,
            trust,
        };
        let answered = fact::row(&fact, Some(&a.session_id), None)?;
        let proposal = match approve {
            true => Some(crate::toolrun::confirm_proposal(&self.store, &a, None)?),
            false => None,
        };
        // A declined layer-1 task change clears its proposal in the answer's
        // frame, under the task's lock (39a): the session, the task, then the
        // execution.
        let task_lock = (!approve)
            .then(|| crate::task_graph::tools::lock_for_answer(&self.store, &a))
            .flatten();
        let task_rec = self.session_rec(&a.session_id);
        let mut declined = None;
        let mut answer = |hold: Option<SessionRecord>| {
            self.kernel.frame(&[&a.execution_id], |k| {
                if task_lock.is_some() {
                    if let Some((records, c)) =
                        crate::task_graph::tools::declined(&self.store, &task_rec, &a)?
                    {
                        k.stage(&records)?;
                        declined = Some(c);
                    }
                }
                match &proposal {
                    Some(p) => {
                        k.bind_confirm(correlation_id, OPERATOR, p)?;
                    }
                    None => {
                        k.decline_action(
                            correlation_id,
                            OPERATOR,
                            note.unwrap_or("the operator declined"),
                        )?;
                    }
                }
                let trusted = match hold {
                    Some(rec) => super::trust::trusted(
                        rec,
                        &a.session_id,
                        &who,
                        theseus_protocol::method::ACTION_CONFIRM,
                        Some(correlation_id),
                    )?,
                    None => None,
                };
                if let Some((_, frame)) = &trusted {
                    k.stage(frame)?;
                }
                k.stage(std::slice::from_ref(&answered))?;
                // The wake is its own part: one that cannot happen (the
                // execution ended) takes back nothing else.
                let why = if approve { "confirmed" } else { "declined" };
                let woke = k
                    .frame(&[&a.execution_id], |k| k.wake(&a.execution_id, why))
                    .is_ok();
                Ok((woke, trusted.map(|(r, _)| r)))
            })
        };
        let (woke, trusted) = match trust {
            true => match self
                .store
                .with_session(&a.session_id, |rec| answer(Some(rec)))?
            {
                Some(done) => done,
                None => answer(None)?,
            },
            false => answer(None)?,
        };
        drop(task_lock);
        if let Some(c) = &declined {
            crate::fact::task_graph::announce(&task_rec, "change_declined", c);
        }
        if let Some(r) = &trusted {
            self.announce_trust(&a.session_id, r);
        }
        // Its row rode in the answer's frame; the rest is said now.
        let rec = self.session_rec(&a.session_id);
        rec.announce(&fact);
        if woke {
            rec.record(&fact::answer::WokenByAnswer { approve });
        }
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

    /// The driver's tick (theseus-830): the questions are read once after
    /// the start, and then only once the earliest may have expired
    /// (`question_due`), never by the executions the tick reads. Returns
    /// how many expired.
    pub fn expire_questions_if_due(&self) -> usize {
        let now = self.kernel.now_ms();
        let due = self
            .tools
            .question_due
            .load(std::sync::atomic::Ordering::SeqCst);
        if now < due {
            return 0;
        }
        self.expire_questions(now)
    }

    /// Expire each call's question nobody answered by `now` (theseus-830):
    /// the time its card and `confirm.list` give, its plan's time and
    /// `[kernel] confirm_ttl_secs`. A budget question holds until it is
    /// answered. Then the earliest one still waiting is when the driver
    /// reads them again. Returns how many expired.
    pub fn expire_questions(&self, now: u64) -> usize {
        use std::sync::atomic::Ordering::SeqCst;
        let ttl = self.kernel.config().confirm_ttl_ms;
        // Raised first: a question asked while these are read lowers it
        // again, so none is missed.
        self.tools.question_due.store(u64::MAX, SeqCst);
        let pending = match self.kernel.pending_confirms() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the questions could not be read for their expiry");
                self.tools.question_due.fetch_min(now + 60_000, SeqCst);
                return 0;
            }
        };
        let mut next = u64::MAX;
        let mut expired = 0;
        // A budget question holds until it is answered.
        for a in pending.iter().filter(|a| a.tool != BUDGET_TOOL) {
            let due = a.planned_at_ms + ttl;
            if due > now {
                next = next.min(due);
                continue;
            }
            match self.expire_question(a, ttl) {
                Ok(()) => expired += 1,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), correlation_id = %a.correlation_id, "a question nobody answered could not expire");
                    next = next.min(now + 60_000);
                }
            }
        }
        self.tools.question_due.fetch_min(next, SeqCst);
        expired
    }

    /// One question's expiry: the decline, its row, and the wake in one
    /// frame, as an answer's are (theseus-jj9f); then its clients hear it,
    /// and its card settles `expired`.
    fn expire_question(&self, a: &Action, ttl_ms: u64) -> Result<()> {
        // A proposed extension's ack: declined by expiry, waking nothing.
        if a.tool == crate::extend::ACK {
            let why = format!("nobody answered within {}", fact::answer::within(ttl_ms));
            let answer = crate::extend::answer::Answer::Expired(&why);
            self.answer_extension(a, answer, EXPIRY, EXPIRY)?;
            return Ok(());
        }
        let fact = fact::answer::QuestionExpired {
            action: a,
            waited_ms: ttl_ms,
        };
        let row = fact::row(&fact, Some(&a.session_id), None)?;
        let why = format!("nobody answered within {}", fact::answer::within(ttl_ms));
        self.kernel.frame(&[&a.execution_id], |k| {
            k.decline_action(&a.correlation_id, EXPIRY, &why)?;
            k.stage(std::slice::from_ref(&row))?;
            // The wake is its own part, as an answer's: one that cannot
            // happen (the execution ended) takes back nothing else.
            let _ = k.frame(&[&a.execution_id], |k| k.wake(&a.execution_id, "expired"));
            Ok(())
        })?;
        self.session_rec(&a.session_id).announce(&fact);
        self.card_closed(
            &a.correlation_id,
            Closed {
                note: Some(fact::answer::within(ttl_ms)),
                ..Closed::new("expired", None)
            },
        );
        self.admission.notify_waiters();
        Ok(())
    }

    /// The one judgment of every approval-like act (theseus-sgh): an answer
    /// to a waiting call (the spend reset among them), a "should have asked"
    /// press, its undo, a trust, and a publish.
    ///
    /// Each but the press takes the place rule: the owner, from a private
    /// place (`places::owner_in_private`, theseus-zmgb). A press only makes
    /// calls ask, so any known surface may make one. Who asks is not traced
    /// (theseus-zmgb): the CLI refuses these acts inside a Theseus job, a
    /// speed bump, and L1, whose view has no route to the daemon, is the
    /// boundary.
    ///
    /// A refusal is ledgered as `approval.refused` (who, through what, and
    /// why) and narrated, and it is the error, a `Refusal`. Nothing else
    /// changes.
    pub(crate) fn judge_act(&self, who: &Answerer, act: Act<'_>) -> Result<()> {
        let verdict = match act {
            Act::Tighten { .. } => who.unknown().map_or(Ok(()), Err),
            _ => crate::places::owner_in_private(who, &self.runner.place_rule, &self.cfg),
        };
        let Err(why) = verdict else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why,
        };
        self.refused(act, who, &r)?;
        Err(r.into())
    }

    /// An act that does not count: ledgered with who, where, and why, and
    /// narrated. A refused answer leaves the action and its execution
    /// waiting; a refused undo leaves the tool asking.
    pub(super) fn refused(&self, act: Act<'_>, who: &Answerer, r: &Refusal) -> Result<()> {
        let fact = fact::answer::ActRefused {
            act,
            refusal: r,
            by: &who.label,
        };
        let session = fact.session();
        self.store.append(&[fact::row(&fact, session, None)?])?;
        let rec = fact::Rec {
            narrator: &self.narrator,
            session,
            turn: None,
            to: fact::To::Everyone(&self.bus),
            store: &self.store,
        };
        rec.announce(&fact);
        Ok(())
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
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let correlation_id = q.correlation_id.as_str();
        // The answer and its row in one frame, a kernel transaction, as a
        // tool call's answer is (theseus-jj9f).
        let fact = fact::answer::BudgetAnswered {
            question: q,
            approve,
            note,
            by,
            via,
        };
        let answered = fact::row(&fact, Some(&q.session_id), None)?;
        let reset = self.kernel.frame(&[&q.execution_id], |k| {
            let reset = match approve {
                true => Some(k.reset_budget(correlation_id, by)?),
                false => {
                    k.decline_action(
                        correlation_id,
                        by,
                        note.unwrap_or("the operator declined the reset"),
                    )?;
                    None
                }
            };
            k.stage(std::slice::from_ref(&answered))?;
            Ok(reset)
        })?;
        let rec = self.session_rec(&q.session_id);
        match reset {
            Some((e, before)) => rec.record(&fact::answer::SpendReset {
                by,
                before,
                limit: e.budget.limit_micros,
            }),
            None => rec.record(&fact::answer::ResetDeclined { by }),
        }
        // Its row rode in the answer's frame.
        rec.announce(&fact);
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
    /// Say what following the spend limit did (theseus-3pj, the kernel's
    /// startup step 2): one log line, a narrative
    /// line per session, and `confirm.resolved` to the clients of each
    /// session whose question a raise withdrew.
    pub(crate) fn said_limits_followed(&self, followed: &[LimitFollowed]) {
        self.said_limits_followed_for(followed, None);
    }

    /// The same, for the limit `place`'s ceiling chose (step 38a), or the
    /// config's with none.
    pub(crate) fn said_limits_followed_for(&self, followed: &[LimitFollowed], place: Option<&str>) {
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
            place,
            "open sessions follow the spend limit"
        );
        for f in followed {
            let rec = self.session_rec(&f.session_id);
            rec.record(&fact::answer::LimitChanged { followed: f, place });
            if let Some(q) = &f.withdrew {
                rec.record(&fact::answer::QuestionWithdrawn {
                    session_id: &f.session_id,
                    question: q,
                });
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
    /// The owner's publish into a place (the place rule): it puts the owner's
    /// material where others read it.
    Publish { place: &'a str },
    /// An ontology write (theseus-8kk.1): guidance steers every session in
    /// its category, so a job's process that wrote it would be an injection
    /// path. `what` names the write: `guidance of topic:theseus`.
    Ontology { method: &'static str, what: &'a str },
    /// A label on a recalled node (M6 30b, `memory.label`): `wrong` keeps
    /// it out of every session's recall, so a job's process that wrote it
    /// would grade its own memory. `what` names it: `wrong on msg_…`.
    Label { what: &'a str },
    /// A label on a judgment (M5 25c, `judge.label`): the learning ledger
    /// grades Jev by it, so a job's process that wrote it would grade the
    /// judge watching it. `what` names it: `wrong on jdg_…`.
    JudgeLabel { what: &'a str },
    /// A loaded extension's revoke (M7 43b, `extension.revoke`): it stops a
    /// server the owner acked, so a job's process that made it would undo
    /// the owner's word.
    Revoke { name: &'a str },
}

impl Act<'_> {
    /// The method that makes the act, as a refusal row names it.
    pub(crate) fn method(self) -> &'static str {
        match self {
            Act::Answer { .. } => theseus_protocol::method::ACTION_CONFIRM,
            Act::Tighten { .. } => theseus_protocol::method::POLICY_TIGHTEN,
            Act::Untighten { .. } => theseus_protocol::method::POLICY_UNTIGHTEN,
            Act::Trust { .. } => theseus_protocol::method::POLICY_TRUST,
            Act::Publish { .. } => theseus_protocol::method::PLACE_PUBLISH,
            Act::Ontology { method, .. } => method,
            Act::Label { .. } => theseus_protocol::method::MEMORY_LABEL,
            Act::JudgeLabel { .. } => theseus_protocol::method::JUDGE_LABEL,
            Act::Revoke { .. } => theseus_protocol::method::EXTENSION_REVOKE,
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
