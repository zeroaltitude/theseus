//! kernel-sim's `/stop` (W1; theseus-celu.35): the operator stops an open
//! conversation between turns (`stop_execution`), a turn stops while it runs,
//! the racing thread stops a raced turn's execution, and one running call is
//! stopped alone (`stop_call`), with crashes around each. A stop is not a
//! cancel: the execution stays open, with its session, budget, spend, pending
//! wakes and tasks; every job and call it had running is told to stop and
//! settles; everything planned and unsent, and the question open on it, is
//! declined; a running turn plans nothing more, and its end parks the
//! execution on input; its next input runs a turn.

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use rand::Rng;
use serde_json::json;
use theseus_kernel::*;

use super::World;

/// What a stop must leave as it was: the state of each of its tasks, and its
/// wakes with their due times.
type Kept = (Vec<(String, ExecState)>, Vec<(String, u64)>);

fn kept(k: &Kernel, e: &Execution) -> Result<Kept> {
    let tasks = k
        .tasks(Some(&e.id))?
        .into_iter()
        .map(|t| (t.id, t.state))
        .collect();
    let wakes = e
        .wakes
        .iter()
        .map(|w| (w.id.clone(), w.due_at_ms))
        .collect();
    Ok((tasks, wakes))
}

fn is_stopped(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::Stopped { .. })
    )
}

impl World {
    /// The operator's `/stop` of an open conversation, between its turns; a
    /// third of the time one running call alone (`stop_call`), and now and
    /// then a task, which is refused.
    pub(super) fn stop_one(&mut self) -> Result<()> {
        if self.chance(0.3) {
            return self.stop_a_call();
        }
        let open = self.kernel.open_executions()?;
        if self.chance(0.1) {
            if let Some(t) = open.iter().find(|e| e.kind == SessionKind::Task) {
                let at = self.kernel.store().last_position();
                match self.kernel.stop_execution(&t.id, "sim") {
                    Err(e)
                        if matches!(
                            e.downcast_ref::<KernelError>(),
                            Some(KernelError::StopTask { .. })
                        ) => {}
                    other => bail!("a stop of task {} was not refused: {other:?}", t.id),
                }
                if self.kernel.store().last_position() != at {
                    bail!("a refused stop of task {} wrote", t.id);
                }
                self.rep.sim2.task_stops_refused += 1;
                return Ok(());
            }
        }
        let convs: Vec<&Execution> = open
            .iter()
            .filter(|e| e.kind == SessionKind::Conversation && e.parent.is_none())
            .collect();
        if convs.is_empty() {
            return Ok(());
        }
        let before = convs[self.rng.random_range(0..convs.len())].clone();
        let stop = self.stop_and_check(&before, "sim")?;
        let id = before.id.clone();
        let crashed = self.maybe_crash("after a stop")?;
        // The operator's daemon terminates what the stop told to stop, after
        // a crash too: its next start finds them asked to stop.
        self.kill_jobs(&stop.to_kill)?;
        self.check_told_settled(&stop.to_kill)?;
        if !crashed && !stop.turn_running && self.chance(0.5) {
            self.input_after_a_stop(&id, &stop.to_kill)?;
        }
        Ok(())
    }

    /// `stop_execution` on `before` (as read just now), and what it must have
    /// done.
    fn stop_and_check(&mut self, before: &Execution, by: &str) -> Result<Stop> {
        let id = &before.id;
        let unsent: BTreeSet<String> = self
            .kernel
            .unsettled_actions(id)?
            .into_iter()
            .filter(|a| matches!(a.state, ActionState::Planned | ActionState::Authorized))
            .map(|a| a.correlation_id)
            .collect();
        let mut running = BTreeSet::new();
        for c in &before.outstanding {
            if let Some(a) = self.kernel.action(c)? {
                if a.state == ActionState::Dispatched
                    && a.cancel.is_none()
                    && a.tool != PROVIDER_TOOL
                {
                    running.insert(a.correlation_id);
                }
            }
        }
        let (tasks, wakes) = kept(&self.kernel, before)?;
        let Some(stop) = self.kernel.stop_execution(id, by)? else {
            bail!("a stop of open {id} ({:?}) wrote nothing", before.state);
        };
        self.rep.sim2.stops += 1;
        self.rep.sim2.stops_in_turn += u64::from(stop.turn_running);
        self.rep.sim2.stopped_calls += stop.to_kill.len() as u64;
        self.rep.sim2.stopped_declined += stop.declined.len() as u64;
        self.s2.stopped_since_scan.insert(id.clone());
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("{id} is gone after its stop"))?;
        if e.state.is_terminal() || self.cancelled.contains(id) {
            bail!("a stop ended {id}: {:?}", e.state);
        }
        if stop.turn_running != (before.state == ExecState::Running) {
            bail!(
                "a stop of {id} ({:?}) says a turn ran: {}",
                before.state,
                stop.turn_running
            );
        }
        match (stop.turn_running, e.state, &e.wake, &e.stopped) {
            (true, ExecState::Running, None, Some(_)) => {}
            (false, ExecState::Waiting, Some(Wake::Input), None) => {}
            other => bail!("a stop left {id} as {other:?}"),
        }
        let b = (&before.budget, &e.budget);
        if e.session_id != before.session_id
            || b.0.limit_micros != b.1.limit_micros
            || b.0.spent_micros != b.1.spent_micros
            || b.1.question.is_some()
        {
            bail!(
                "a stop of {id} changed its session or budget: {:?} -> {:?}",
                b.0,
                b.1
            );
        }
        if kept(&self.kernel, &e)? != (tasks, wakes) {
            bail!("a stop of {id} changed its tasks or its wakes");
        }
        let declined: BTreeSet<String> = stop
            .declined
            .iter()
            .map(|a| a.correlation_id.clone())
            .collect();
        if declined != unsent {
            bail!("a stop of {id} declined {declined:?}, and {unsent:?} were unsent");
        }
        for c in &declined {
            let a = self.kernel.action(c)?.unwrap();
            if a.state != ActionState::Cancelled {
                bail!("a stop of {id} declined {c}, and it is {:?}", a.state);
            }
        }
        let told: BTreeSet<String> = stop.to_kill.iter().cloned().collect();
        if told != running {
            bail!("a stop of {id} told {told:?} to stop, and {running:?} ran");
        }
        for c in &told {
            let a = self.kernel.action(c)?.unwrap();
            if a.cancel != Some(CancelState::Requested) {
                bail!("a stop of {id} told {c} to stop, and it is {:?}", a.cancel);
            }
        }
        Ok(stop)
    }

    /// Every call a stop told to stop has settled once its backend was
    /// terminated.
    pub(super) fn check_told_settled(&self, told: &[String]) -> Result<()> {
        for c in told {
            if let Some(a) = self.kernel.action(c)? {
                if !a.state.is_settled() {
                    bail!("{c} was told to stop and is {:?} after its kill", a.state);
                }
            }
        }
        Ok(())
    }

    /// The stopped conversation's next input runs a turn, which reads each
    /// stopped call as a result.
    fn input_after_a_stop(&mut self, id: &str, told: &[String]) -> Result<()> {
        let g = match self.kernel.admit_input(id) {
            Ok(g) => g,
            Err(err)
                if matches!(
                    err.downcast_ref::<KernelError>(),
                    Some(KernelError::AdmissionFull { .. })
                ) =>
            {
                return Ok(())
            }
            Err(err) => bail!("the input after a stop of {id} ran no turn: {err:#}"),
        };
        self.rep.turns += 1;
        self.rep.input_admits += 1;
        self.rep.sim2.stops_resumed += 1;
        let taken: BTreeSet<String> = self
            .kernel
            .take_results(&g)?
            .into_iter()
            .map(|a| a.correlation_id)
            .collect();
        for c in told {
            if !taken.contains(c) {
                bail!("the turn after a stop of {id} did not read its stopped call {c}");
            }
        }
        self.end_turn_posting(g, TurnEnd::Wait { wake: Wake::Input })?;
        Ok(())
    }

    /// The operator stops one running call alone (`stop_call`): its
    /// execution is left as it was, and a second stop writes nothing.
    fn stop_a_call(&mut self) -> Result<()> {
        let running: Vec<Action> = self
            .kernel
            .open_actions()?
            .into_iter()
            .filter(|a| {
                a.state == ActionState::Dispatched && a.cancel.is_none() && a.tool != PROVIDER_TOOL
            })
            .collect();
        if running.is_empty() {
            return Ok(());
        }
        let a = running[self.rng.random_range(0..running.len())].clone();
        let before = self.kernel.execution(&a.execution_id)?;
        let Some(stopped) = self.kernel.stop_call(&a.correlation_id, "sim")? else {
            bail!("a stop of running call {} wrote nothing", a.correlation_id);
        };
        if stopped.cancel != Some(CancelState::Requested)
            || stopped.resolution.as_deref() != Some("stopped by sim")
        {
            bail!("a stopped call reads {stopped:?}");
        }
        let after = self.kernel.execution(&a.execution_id)?;
        if json!(before) != json!(after) {
            bail!(
                "a stop of call {} changed its execution {}",
                a.correlation_id,
                a.execution_id
            );
        }
        let at = self.kernel.store().last_position();
        if self.kernel.stop_call(&a.correlation_id, "sim")?.is_some()
            || self.kernel.store().last_position() != at
        {
            bail!("a second stop of call {} wrote", a.correlation_id);
        }
        self.rep.sim2.call_stops += 1;
        self.maybe_crash("after a call's stop")?;
        let told = [a.correlation_id];
        self.kill_jobs(&told)?;
        self.check_told_settled(&told)
    }

    /// A stop lands while the turn holding `exec_id` runs: the turn plans
    /// nothing more, and its end, whatever it would have been, parks the
    /// execution on input. A crash may come after the stop, and the start
    /// after it parks it so too.
    pub(super) fn stop_in_turn(&mut self, exec_id: &str) -> Result<()> {
        let before = self.kernel.execution(exec_id)?.unwrap();
        let stop = self.stop_and_check(&before, "sim")?;
        let g = self.guards.get(exec_id).unwrap();
        let prop = Proposal {
            tool: "fake.fast".into(),
            args: json!({"after": "stop"}),
            resource: None,
            policy_context: json!({}),
        };
        let at = self.kernel.store().last_position();
        let planned = self
            .kernel
            .plan_action(g, &prop, RetryClass::SafeToRepeat, Some(20_000), 0);
        let set =
            self.kernel
                .set_wake(g, "act_after_stop", self.now() + 1_000, "after", None, None);
        match (planned, set) {
            (Err(p), Err(s)) if is_stopped(&p) && is_stopped(&s) => {}
            (p, s) => bail!(
                "a stopped turn of {exec_id} planned: {:?} / {:?}",
                p.map(|a| a.correlation_id),
                s.map(|w| w.wake.id)
            ),
        }
        if self.kernel.store().last_position() != at {
            bail!("a stopped turn of {exec_id} wrote a refused plan");
        }
        if self.maybe_crash("after a stop in a turn")? {
            self.kill_jobs(&stop.to_kill)?;
            let e = self.kernel.execution(exec_id)?.unwrap();
            if !e.state.is_terminal()
                && !(e.state == ExecState::Waiting && e.wake == Some(Wake::Input))
            {
                bail!(
                    "a turn stopped before a crash left {exec_id} {:?} {:?}",
                    e.state,
                    e.wake
                );
            }
            return Ok(());
        }
        self.kill_jobs(&stop.to_kill)?;
        self.check_told_settled(&stop.to_kill)?;
        let end = match self.rng.random_range(0..4) {
            0 => TurnEnd::Complete {
                reason: "sim done".into(),
            },
            1 => TurnEnd::Wait {
                wake: Wake::DueAt {
                    at_ms: self.now() + 5_000,
                },
            },
            2 => TurnEnd::Fail {
                reason: "sim fault".into(),
            },
            _ => TurnEnd::Requeue,
        };
        let g = self.guards.remove(exec_id).unwrap();
        let e = self.end_turn_posting(g, end)?;
        if !(e.state == ExecState::Waiting && e.wake == Some(Wake::Input) && e.stopped.is_none()) {
            bail!(
                "a stopped turn's end left {exec_id} {:?} {:?}",
                e.state,
                e.wake
            );
        }
        Ok(())
    }

    /// A ledger row, in WAL order, for the stop's rule: after a stop that
    /// landed while a turn ran, that turn plans nothing (no call, wake or
    /// task), and it ends parked on input (`why: stopped`), interrupted by a
    /// crash (the start parks it so), or cancelled.
    pub(super) fn stop_row(&mut self, at: &str, id: &str, row: &LedgerRow, pos: u64) -> Result<()> {
        let kind = row.kind.as_str();
        if kind == "execution.stopped" {
            if row.data["turn_running"].as_bool() == Some(true) {
                let turn = row.data["turn"].as_u64().unwrap_or(0);
                self.s2.stopped_turn.entry(id.to_string()).or_insert(turn);
                if row.data["state_before"] != "running" {
                    self.s2.stopped_held.insert(id.to_string());
                }
            }
            return Ok(());
        }
        let Some(&turn) = self.s2.stopped_turn.get(id) else {
            return Ok(());
        };
        // A held turn's admission, after the stop that marked it.
        if self.s2.stopped_held.contains(id) {
            match kind {
                "execution.queued" if row.data["why"] == "input" => return Ok(()),
                "execution.running" => {
                    self.s2.stopped_held.remove(id);
                    return Ok(());
                }
                _ => {}
            }
        }
        match kind {
            "action.planned" if row.data["tool"] != OUTBOX_TOOL => {}
            "wake.set" | "budget.carved" => {}
            "execution.waiting" if row.data["why"] == "stopped" => {
                self.s2.stopped_turn.remove(id);
                self.s2.stopped_held.remove(id);
                return Ok(());
            }
            "execution.interrupted" if !row.data["stopped_by"].is_null() => {
                self.s2.stopped_turn.remove(id);
                self.s2.stopped_held.remove(id);
                return Ok(());
            }
            "execution.cancelled" => {
                self.s2.stopped_turn.remove(id);
                self.s2.stopped_held.remove(id);
                return Ok(());
            }
            "execution.waiting"
            | "execution.queued"
            | "execution.running"
            | "execution.interrupted"
            | "execution.complete"
            | "execution.failed"
            | "execution.blocked" => {}
            _ => return Ok(()),
        }
        bail!(
            "{at}: {id} was stopped in its turn {turn}, and then wrote {kind} at {pos}: {}",
            row.data
        )
    }
}
