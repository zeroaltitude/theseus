//! kernel-sim's tasks under a parent (DD7, W1, 37b; theseus-celu.35): a
//! conversation's turn opens a task (`open_task`), with and without
//! `wake_parent`, carved from what the parent has left; the task takes its
//! own turns, sets its own wakes, and ends, fails, or is cancelled; the
//! parent's turns read the reports (`take_reports`). Each report reaches its
//! parent once, across crashes, never lost or twice; a report's wake asks for
//! the parent's turn, which a busy parent keeps until the frame that frees
//! it, and reports that land together start one turn; a cancelled task wakes
//! nothing; no session outspends its limit through its tasks (the budget
//! check over every execution), and a carve is what its task can still
//! spend: its room while it runs, what it has in flight once it has ended.

use anyhow::{bail, Result};
use rand::Rng;
use theseus_kernel::tasks::carve_key;
use theseus_kernel::wakes::{free, task_unparked};
use theseus_kernel::*;

use super::World;

impl World {
    /// The turn holding `exec_id` opens a task. A task's own turn is refused
    /// (depth one), and writes nothing; a parent with nothing left to carve
    /// opens none. Returns whether a crash came after it.
    pub(super) fn open_a_task(&mut self, exec_id: &str) -> Result<bool> {
        let parent = self.kernel.execution(exec_id)?.unwrap();
        let corr = format!("act_task{}", self.rng.random::<u64>());
        let want = self.rng.random_range(500..4_000);
        let wake_parent = self.chance(0.5);
        let at = self.kernel.store().last_position();
        let g = self.guards.get(exec_id).unwrap();
        let opened = self.kernel.open_task(
            g,
            &corr,
            want,
            Some(format!("reports:{exec_id}")),
            wake_parent,
            |_| Ok(vec![]),
        );
        let refused = |e: &anyhow::Error, want_depth: bool| {
            matches!(
                (e.downcast_ref::<KernelError>(), want_depth),
                (Some(KernelError::TaskDepth { .. }), true)
                    | (Some(KernelError::NothingToCarve { .. }), false)
            )
        };
        let a_task = parent.kind == SessionKind::Task || parent.parent.is_some();
        let t = match opened {
            Err(e) if refused(&e, a_task) => {
                if self.kernel.store().last_position() != at {
                    bail!("a refused task open from {exec_id} wrote");
                }
                self.rep.sim2.depth_refused += u64::from(a_task);
                return Ok(false);
            }
            Err(e) => return Err(e),
            Ok(_) if a_task => bail!("{exec_id}, a task, opened a task"),
            Ok(t) => t,
        };
        let task = &t.task;
        let limit = want.min(t.available_before);
        if !t.opened
            || task.parent.as_deref() != Some(exec_id)
            || task.budget.limit_micros != limit
            || !task.budget.pinned
            || task.wake_parent != wake_parent
        {
            bail!("{exec_id} opened task {} as {task:?}", task.id);
        }
        self.pinned.insert(task.id.clone(), limit);
        self.s2.tasks.insert(task.id.clone(), exec_id.to_string());
        self.rep.sim2.tasks_opened += 1;
        self.rep.sim2.tasks_waking += u64::from(wake_parent);
        if self.maybe_crash("after open_task")? {
            return Ok(true);
        }
        // The same call run again finds its task, and writes nothing.
        if self.chance(0.2) {
            let at = self.kernel.store().last_position();
            let g = self.guards.get(exec_id).unwrap();
            let again = self
                .kernel
                .open_task(g, &corr, want, None, wake_parent, |_| Ok(vec![]))?;
            if again.opened || again.task.id != task.id || self.kernel.store().last_position() != at
            {
                bail!("{corr}, run again, opened {:?}", again.task.id);
            }
            self.rep.sim2.tasks_found_again += 1;
        }
        Ok(false)
    }

    /// A task under a parent that waits on nothing it sent finishes, or
    /// fails, two times in five, so its report (and a report's wake) reaches
    /// its parent within a run.
    pub(super) fn task_end(&mut self, e: &Execution, end: TurnEnd) -> TurnEnd {
        if e.parent.is_none()
            || matches!(
                end,
                TurnEnd::Wait {
                    wake: Wake::Actions { .. }
                }
            )
        {
            return end;
        }
        if !self.chance(0.4) {
            return end;
        }
        if self.chance(0.25) {
            TurnEnd::Fail {
                reason: "sim task failed".into(),
            }
        } else {
            TurnEnd::Complete {
                reason: "sim task done".into(),
            }
        }
    }

    /// The turn holding `exec_id` reads its tasks' reports: each once, of a
    /// task of its own that has ended, and a wake only of one that asked and
    /// was not cancelled. Returns whether a crash came after it.
    pub(super) fn take_reports(&mut self, exec_id: &str) -> Result<bool> {
        let g = self.guards.get(exec_id).unwrap();
        let taken = self.kernel.take_reports(g, |_| Ok(vec![]))?;
        if taken.ids.is_empty() && taken.woke.is_empty() {
            return Ok(false);
        }
        for id in &taken.ids {
            if self.s2.tasks.get(id).map(String::as_str) != Some(exec_id) {
                bail!("{exec_id} read a report of {id}, not a task of its own");
            }
            let t = self.kernel.execution(id)?.unwrap();
            if !t.state.is_terminal() {
                bail!("{exec_id} read a report of {id}, which is {:?}", t.state);
            }
            if !self.s2.reports_taken.insert(id.clone()) {
                bail!("{id}'s report reached {exec_id} twice");
            }
        }
        for id in &taken.woke {
            let t = self.kernel.execution(id)?.unwrap();
            if !taken.ids.contains(id) || !t.wake_parent || t.state == ExecState::Cancelled {
                bail!(
                    "{id} woke {exec_id}: {:?}, wake_parent {}",
                    t.state,
                    t.wake_parent
                );
            }
        }
        self.rep.sim2.reports_read += taken.ids.len() as u64;
        self.rep.sim2.report_wakes += taken.woke.len() as u64;
        self.rep.sim2.reports_together += u64::from(taken.ids.len() > 1);
        let e = self.kernel.execution(exec_id)?.unwrap();
        if !e.reports.is_empty() || !e.report_wakes.is_empty() {
            bail!(
                "{exec_id} read its reports and still holds {:?} / {:?}",
                e.reports,
                e.report_wakes
            );
        }
        self.maybe_crash("after take_reports")
    }

    /// Every task and its parent, at every check: depth one; a carve is what
    /// its task can still spend; each ended task's report waits on its open
    /// parent until a turn read it, and only then is gone; a wake only of a
    /// task that ended and asked, never a cancelled one; a free parent holds
    /// no report's wake unless a stop freed it since the last due scan; and
    /// no task waits on input with nothing to wake it.
    pub(super) fn check_tasks(&self, at: &str, execs: &[Execution]) -> Result<()> {
        let by_id: std::collections::HashMap<&str, &Execution> =
            execs.iter().map(|e| (e.id.as_str(), e)).collect();
        for t in execs {
            let Some(pid) = &t.parent else { continue };
            let p = by_id
                .get(pid.as_str())
                .ok_or_else(|| anyhow::anyhow!("{at}: task {}'s parent {pid} is gone", t.id))?;
            if p.parent.is_some() || t.kind != SessionKind::Task {
                bail!("{at}: task {} under {pid} is more than one deep", t.id);
            }
            if !t.state.is_terminal() && task_unparked(t) {
                bail!("{at}: task {} waits on input with no wake of its own", t.id);
            }
            if p.state.is_terminal() {
                continue;
            }
            let b = &t.budget;
            let still = if t.state.is_terminal() {
                b.reserved_micros + b.held_unknown_micros
            } else {
                b.limit_micros.saturating_sub(b.spent_micros)
            };
            let carve = p
                .budget
                .reservations
                .get(&carve_key(&t.id))
                .copied()
                .unwrap_or(0);
            if carve != still {
                bail!(
                    "{at}: {pid}'s carve for {} ({:?}) is {carve}, and it can still spend {still}: {b:?}",
                    t.id,
                    t.state
                );
            }
            let taken = self.s2.reports_taken.contains(&t.id);
            let reported = p.reports.contains(&t.id);
            if t.state.is_terminal() && taken == reported {
                bail!(
                    "{at}: {}'s report ({:?}) {} {pid}",
                    t.id,
                    t.state,
                    if taken {
                        "was read and is still on"
                    } else {
                        "never reached"
                    }
                );
            }
            if !t.state.is_terminal() && reported {
                bail!("{at}: {} is {:?} and on {pid}'s reports", t.id, t.state);
            }
            let woke = p.report_wakes.contains(&t.id);
            let asks =
                t.state.is_terminal() && t.wake_parent && t.state != ExecState::Cancelled && !taken;
            if woke != asks {
                bail!(
                    "{at}: {} ({:?}, wake_parent {}, read {taken}) {} on {pid}'s report wakes",
                    t.id,
                    t.state,
                    t.wake_parent,
                    if woke { "is" } else { "is not" }
                );
            }
        }
        for p in execs {
            if !p.state.is_terminal()
                && !p.report_wakes.is_empty()
                && free(p)
                && !self.s2.stopped_since_scan.contains(&p.id)
            {
                bail!(
                    "{at}: {} holds report wakes {:?} and is parked on {:?}",
                    p.id,
                    p.report_wakes,
                    p.wake
                );
            }
        }
        Ok(())
    }

    /// A ledger row, in WAL order, for the reports' rules: a cancelled task
    /// writes no report's wake, and reports that land together queue their
    /// parent once, so between two queues for a report a turn of it ran.
    pub(super) fn report_row(
        &mut self,
        at: &str,
        id: &str,
        row: &LedgerRow,
        pos: u64,
    ) -> Result<()> {
        match row.kind.as_str() {
            "task.report_wake" if row.data["state"] == "cancelled" => bail!(
                "{at}: cancelled task {} woke {id} at {pos}",
                row.data["task"]
            ),
            "execution.queued" if row.data["why"] == "report" => {
                if !self.s2.report_queued.insert(id.to_string()) {
                    bail!("{at}: {id} was queued twice for its reports, with no turn between, at {pos}");
                }
            }
            // A turn ran, or a stop parked it again: the due scan may queue
            // it for the same reports.
            "execution.running" | "execution.stopped" => {
                self.s2.report_queued.remove(id);
            }
            _ => {}
        }
        Ok(())
    }
}
