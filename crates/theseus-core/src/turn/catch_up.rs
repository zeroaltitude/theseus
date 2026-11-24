//! What a continuation catches up on before the model runs: the wakes that fired, the reports that came in, the late
//! results, and whether any of it is news the model has not read.

use super::*;

impl TurnRunner {
    /// What happened while no turn was running: results that settled become
    /// late result nodes, and pending calls resume (a confirm answered, a
    /// restart survived). Returns how many nodes that wrote.
    pub(super) async fn catch_up(&self, t: &mut Turn<'_>, has_input: bool) -> Result<u32> {
        let t0 = t.trace.now_us();
        let (settled, late) = self.tools.absorb(&t.tc)?;
        let absorbed = late.len() as u32;
        // A budget question the kernel queued as a result: approved (the
        // spend was reset), or withdrawn by a raised limit (theseus-3pj).
        // Either way the call that did not fit proceeds.
        let budget = |approved: bool| {
            settled
                .iter()
                .any(|a| a.tool == BUDGET_TOOL && (a.state == ActionState::Succeeded) == approved)
        };
        let (reset, raised) = (budget(true), budget(false));
        // An approved reset of a call that no reset can fit: it alone needs
        // more than the limit (theseus-kks), or more than the limit leaves
        // once what a reset keeps held is taken (theseus-6g6). The retry gets
        // one try, and asks nothing more.
        t.retry_over_limit = settled.iter().any(|a| {
            a.tool == BUDGET_TOOL
                && a.state == ActionState::Succeeded
                && a.proposal.as_ref().is_some_and(|p| {
                    let needed = p.args["needed_micros"].as_u64().unwrap_or(0);
                    t.tc.kernel
                        .execution(t.tc.execution_id)
                        .ok()
                        .flatten()
                        .is_some_and(|e| needed > e.budget.available_after_reset())
                })
        });
        let resumed = self.tools.resume(&t.tc, has_input).await?;
        // A hold's yes that no call of this turn's resume took is dropped (issue 28).
        self.tools.profile.drop_untaken(t.tc.session_id, t.arrived);
        // The reports of this session's tasks that ended since its last turn
        // (DD7), after the calls above and before the new input.
        let reported = self.read_reports(t)?;
        // Its wakes that are due (DD8), last: each is this turn's input, or
        // comes before the input that arrived with it.
        let woke = Self::read_wakes(t)?;
        // A retraction's correction notes (theseus-2ced), none usually.
        self.read_corrections(t)?;
        let deferred = deferred_step::start_node(t)?;
        // Each call answered here is traced under the continuation's span,
        // in this turn, as `run_tools` traces its own (theseus-8pei).
        let mut calls = calls::late_spans(&t.trace, &self.tools, &late);
        calls.extend(calls::call_spans(
            &t.trace,
            &self.tools,
            &resumed.calls,
            &resumed.ran,
        ));
        // What woke a turn no message, wake or report of its own opened: the view says it (issue 44).
        let opened = has_input || reported + woke > 0 || view_step::task::brief_opens(t);
        let occasion =
            view_step::occasion::Occasion::of(opened, &late, resumed.wrote > 0 || reset || raised);
        t.exchange.caught_up(occasion);
        t.record(&fact::turn::CaughtUp {
            t0,
            settled: settled.len(),
            absorbed,
            resumed: &resumed,
            reset,
            raised,
            reported,
            woke,
            calls,
        });
        t.awaiting = resumed.awaiting;
        t.background = resumed.background;
        Ok(absorbed + resumed.wrote + u32::from(reset || raised) + reported + woke + deferred)
    }

    /// A `/stop` by `by` ended the turn's loops (W1): its stop reason says so,
    /// and it posts no reply. Its end parks the execution on input.
    pub(super) fn stopped(t: &mut Turn<'_>, by: &str) {
        t.stop_reason = "stopped".into();
        t.record(&fact::turn::Stopped { by });
    }

    /// The session's wakes that are due (DD8): each becomes a node the model
    /// reads as the user's, `⏰ wake (set 13:05): <note>`, written in the one
    /// frame that removes them from the execution, so none runs twice. None
    /// due, and nothing is written. The reply names them, and goes where the
    /// first of them was set from if the session posts nowhere now.
    fn read_wakes(t: &mut Turn<'_>) -> Result<u32> {
        let tc = &t.tc;
        let mut nodes = Vec::new();
        let (fired, pending) = tc.kernel.take_wakes_listing(tc.guard, |due| {
            let mut records = Vec::new();
            for f in due {
                let n = Node::relayed(
                    tc.session_id,
                    Some(tc.turn_id),
                    crate::node::Origin::Harness,
                    &format!("wake:{}", crate::task::short(&f.wake.id)),
                    &crate::wake::fired_text(f),
                );
                records.push(n.record()?);
                nodes.push(n);
            }
            // Where its reply goes if the session posts nowhere by then,
            // kept past this turn, so a retry of it answers there too
            // (theseus-4lx).
            if let Some(target) = due.iter().find_map(|f| f.wake.target.as_deref()) {
                records.push(tc.outbox.wake_target_record(tc.session_id, target)?);
            }
            Ok(records)
        })?;
        // The commitments block's list, from this read (theseus-lherl).
        tc.commitments.set(pending);
        for n in &nodes {
            tc.node_written(n);
        }
        for f in &fired {
            tc.record(&fact::turn::WakeCameDue { fired: f });
        }
        t.wake_target = fired.iter().find_map(|f| f.wake.target.clone());
        t.wakes = fired
            .iter()
            .map(|f| json!({"wake_id": f.wake.id, "text": crate::wake::fired_text(f), "late_ms": f.late_ms}))
            .collect();
        Ok(fired.len() as u32)
    }

    /// The reports of the tasks this session started that ended since its
    /// last turn (DD7): each becomes a node in this session, read by the
    /// model with whatever came with this turn. One frame clears them from
    /// the execution and writes their nodes; none, and nothing is written.
    /// Those that asked for this turn (W1, `wake_parent`) are named on its
    /// reply's post, and a reply whose session posts nowhere now goes where
    /// the first of them reported.
    fn read_reports(&self, t: &mut Turn<'_>) -> Result<u32> {
        let tc = &t.tc;
        let mut nodes = Vec::new();
        let mut reports = Vec::new();
        // A held turn's admission rides the frame that takes them, when there
        // are any (theseus-2uby).
        let turn = &*t;
        let cuts = tc.scrubber.cuts();
        let taken = tc.kernel.frame(&[tc.execution_id], |k| {
            let e = k.execution(tc.execution_id)?;
            if e.is_some_and(|e| !(e.reports.is_empty() && e.report_wakes.is_empty())) {
                self.admit_in(k, turn)?;
            }
            k.take_reports(tc.guard, |ids| {
                let mut records = Vec::new();
                for id in ids {
                    let Some(r) = crate::task::load_report(&self.store, &self.kernel, id)? else {
                        continue;
                    };
                    // A report is delivered whole where its work reports.
                    let text = r.node_text(&cuts);
                    let n = Node::relayed(
                        tc.session_id,
                        Some(tc.turn_id),
                        crate::node::Origin::Harness,
                        &format!("task:{}", r.short),
                        &text,
                    );
                    records.push(n.record()?);
                    // The first transmission edge (12a, theseus-n4m): the
                    // relayed node copies the task's last message, so
                    // `node.reach` follows it from there. A task that did
                    // not finish relays no message, and has no edge.
                    if let Some(last) = r.node.as_ref() {
                        records.push(
                            crate::graph::Edge::new(
                                crate::graph::EdgeKind::DerivedFrom,
                                &n.id,
                                last,
                                crate::graph::VIA_REPORT,
                            )
                            .record()?,
                        );
                    }
                    nodes.push(n);
                    reports.push(r);
                }
                // Where the reply to them goes if the session posts nowhere
                // by then, kept past this turn (theseus-4lx).
                if let Some(target) = reports.iter().find_map(|r| r.target.as_deref()) {
                    records.push(tc.outbox.wake_target_record(tc.session_id, target)?);
                }
                Ok(records)
            })
        })?;
        for n in &nodes {
            tc.node_written(n);
        }
        self.content_nodes(tc.session_id, &nodes, ItemKind::TaskReport, "task report");
        for r in reports
            .iter()
            .filter(|r| taken.woke.contains(&r.execution_id))
        {
            tc.record(&fact::turn::ReportWoke { report: r });
            t.woke_by.push(json!({"task": r.task, "short": r.short,
                "text": crate::task::woke_line(&r.short), "outcome": r.outcome}));
            if t.wake_target.is_none() {
                t.wake_target = r.target.clone();
            }
        }
        Ok(nodes.len() as u32)
    }

    /// Does the model have anything new to read: input or results it has not
    /// seen, or a user message still waiting for a reply? If not, the turn's
    /// stop reason says why. Only a continuation that wrote nothing asks the
    /// transcript, and what it finds after the model's last answer is unread
    /// by definition.
    pub(super) fn has_news(t: &mut Turn<'_>, wrote: bool) -> Result<bool> {
        if t.awaiting.is_some() {
            t.stop_reason = "awaiting_confirm".into();
            t.record(&fact::turn::NoCall::AwaitingApproval);
            return Ok(false);
        }
        if wrote {
            return Ok(true);
        }
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let last = nodes
            .iter()
            .rev()
            .find(|(_, n)| !crate::stub::Kind::is_note(n.kind));
        // A recall's note, written by a turn's compile after the message it
        // fed, is no reply and no input, nor is a summary its compaction
        // wrote: a message with only those after it still waits
        // (theseus-posww). Neither is a call, as before.
        // Results the model has not read are its next input (theseus-kol). A
        // continuation that finds them was woken to have them read: the turn
        // that wrote them failed or faulted before its model call returned
        // (its retry), a late result landed as it ended, or a crash cut it.
        // Ending `nothing_new` here once left a job's result unanswered for
        // good. A turn that stopped short of them by its own decision (its
        // loop cap, a `/stop`) parks on input, and nothing wakes it without
        // writing something new. A task goes on by itself as it always did
        // (DD7).
        let awaiting_reply = matches!(
            last.map(|(_, n)| &n.body),
            // A task's arrangement follows its brief (M5 27).
            Some(Body::UserMessage { .. } | Body::ToolResult { .. } | Body::Arrangement { .. })
        );
        if !awaiting_reply {
            t.stop_reason = "nothing_new".into();
            t.record(&fact::turn::NoCall::NothingNew);
        }
        Ok(awaiting_reply)
    }
}
