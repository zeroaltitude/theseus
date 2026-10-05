//! The kernel-sim's wakes (DD8, 37a): turns set them, one-shot and
//! repeating, and take the due ones; the operator cancels some; and every
//! check holds each execution's pending wakes to their rules. A wake may
//! fall due while its turn runs, and the turn's end queues it (sim2); after
//! the heartbeat's due scan, nothing due is left parked.

use anyhow::{bail, Result};
use rand::Rng;
use theseus_kernel::wakes::{due_now, wake_due};
use theseus_kernel::*;

use super::World;

impl World {
    /// The turn holding `exec_id` takes its due wakes (DD8): each taken at
    /// or after its time, no occurrence twice, and each series put back at
    /// the first occurrence after now, or ended by its `until` (37a).
    pub(super) fn take_wakes(&mut self, exec_id: &str) -> Result<()> {
        let g = self.guards.get(exec_id).unwrap();
        let now = self.now();
        let fired = self.kernel.take_wakes(g, |_| Ok(vec![]))?;
        // What the take left pending, read back from the store.
        let pending = self
            .kernel
            .execution(exec_id)?
            .map(|e| e.wakes)
            .unwrap_or_default();
        let zone = &self.kernel.config().zone;
        for f in &fired {
            self.rep.wakes_fired += 1;
            let w = &f.wake;
            if w.due_at_ms > now {
                bail!(
                    "{exec_id}: wake {} taken at {now}, before its time {}",
                    w.id,
                    w.due_at_ms
                );
            }
            if !self.fired.insert((w.id.clone(), w.occurrence)) {
                bail!(
                    "{exec_id}: wake {} occurrence {} ran twice",
                    w.id,
                    w.occurrence
                );
            }
            match (&w.repeat, f.next_due_at_ms) {
                (None, None) if pending.iter().any(|p| p.id == w.id) => {
                    bail!(
                        "{exec_id}: one-shot wake {} is still pending after it ran",
                        w.id
                    )
                }
                (None, None) => {}
                (None, Some(n)) => bail!("{exec_id}: one-shot wake {} put back at {n}", w.id),
                (Some(r), Some(n)) => {
                    self.rep.repeats_rearmed += 1;
                    if n <= now || r.next_after(zone, now) != Some(n) {
                        bail!(
                            "{exec_id}: series {} put back at {n}, not the first after {now}",
                            w.id
                        );
                    }
                    let back = pending.iter().find(|p| p.id == w.id);
                    if back.map(|p| (p.due_at_ms, p.occurrence))
                        != Some((n, w.occurrence + 1 + f.missed as u32))
                    {
                        bail!(
                            "{exec_id}: series {} #{} was taken, and its next is not pending at {n}: {back:?}",
                            w.id,
                            w.occurrence
                        );
                    }
                }
                (Some(r), None) => {
                    self.rep.wakes_ended += 1;
                    if r.until_ms.is_none() || r.next_after(zone, now).is_some() {
                        bail!(
                            "{exec_id}: series {} ended before its until ({:?})",
                            w.id,
                            r.until_ms
                        );
                    }
                }
            }
            self.rep.wakes_missed += f.missed;
        }
        Ok(())
    }

    /// The turn holding `exec_id` sets a wake, half of them repeating every
    /// 1 to 4 minutes, some until a time. At the cap it is refused, and
    /// nothing is written.
    pub(super) fn set_a_wake(&mut self, exec_id: &str) -> Result<()> {
        let now = self.now();
        let due = now + self.rng.random_range(1_000..60_000);
        let repeat = if self.chance(0.5) {
            let every = Every {
                n: self.rng.random_range(1..=4),
                unit: repeat::Unit::Minutes,
            };
            let until_ms = self
                .chance(0.3)
                .then(|| due + self.rng.random_range(0..1_800_000));
            Some(Repeat {
                every,
                first_ms: due,
                days: vec![],
                until_ms,
            })
        } else {
            None
        };
        let repeating = repeat.is_some();
        let call = format!("act_wake{}", self.rng.random::<u64>());
        let g = self.guards.get(exec_id).unwrap();
        match self
            .kernel
            .set_wake(g, &call, due, "sim wake", None, repeat)
        {
            Ok(set) => {
                self.rep.wakes_set += u64::from(set.set);
                self.rep.repeats_set += u64::from(set.set && repeating);
                Ok(())
            }
            Err(err)
                if matches!(
                    err.downcast_ref::<KernelError>(),
                    Some(KernelError::TooManyWakes { .. })
                ) =>
            {
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// The operator cancels a pending wake, one-shot or a series.
    pub(super) fn cancel_a_wake(&mut self) -> Result<()> {
        let pending = self.kernel.pending_wakes()?;
        if pending.is_empty() {
            return Ok(());
        }
        let (e, w) = &pending[self.rng.random_range(0..pending.len())];
        if self.kernel.cancel_wake(&e.id, &w.id, "sim")?.is_some() {
            self.rep.wakes_cancelled += 1;
            self.occurrences.remove(&w.id);
        }
        self.maybe_crash("after cancel_wake")?;
        Ok(())
    }

    /// Every execution's pending wakes (DD8, 37a): at most `MAX_PENDING`,
    /// soonest first, each id once, none on an execution that ended; a
    /// series' due time is one of its occurrences, none past its `until`,
    /// and its occurrence only grows.
    pub(super) fn check_wakes(&mut self, at: &str, execs: &[Execution]) -> Result<()> {
        let zone = self.kernel.config().zone.clone();
        for e in execs {
            if e.state.is_terminal() && !e.wakes.is_empty() {
                bail!(
                    "{at}: {} is {} and holds {} wakes",
                    e.id,
                    e.state.as_str(),
                    e.wakes.len()
                );
            }
            if e.wakes.len() > MAX_PENDING {
                bail!("{at}: {} holds {} wakes", e.id, e.wakes.len());
            }
            let keys: Vec<(u64, &str)> = e
                .wakes
                .iter()
                .map(|w| (w.due_at_ms, w.id.as_str()))
                .collect();
            if !keys.windows(2).all(|p| p[0] < p[1]) {
                bail!(
                    "{at}: {}'s wakes are not soonest first, each once: {keys:?}",
                    e.id
                );
            }
            for w in &e.wakes {
                if let Some(r) = &w.repeat {
                    if w.occurrence == 0
                        || r.next_after(&zone, w.due_at_ms.saturating_sub(1)) != Some(w.due_at_ms)
                    {
                        bail!(
                            "{at}: series {} due at {} (#{}) is not one of its occurrences",
                            w.id,
                            w.due_at_ms,
                            w.occurrence
                        );
                    }
                    let before = self.occurrences.insert(w.id.clone(), w.occurrence);
                    if before.is_some_and(|b| b > w.occurrence) {
                        bail!(
                            "{at}: series {} went back from #{before:?} to #{}",
                            w.id,
                            w.occurrence
                        );
                    }
                } else if w.occurrence != 0 {
                    bail!(
                        "{at}: one-shot wake {} has occurrence {}",
                        w.id,
                        w.occurrence
                    );
                }
            }
        }
        Ok(())
    }

    /// Now and then the turn holding `exec_id` runs long, past its soonest
    /// pending wake: the wake falls due while its execution is busy.
    pub(super) fn run_past_a_wake(&mut self, exec_id: &str) -> Result<()> {
        if !self.chance(0.15) {
            return Ok(());
        }
        let Some(e) = self.kernel.execution(exec_id)? else {
            return Ok(());
        };
        let now = self.now();
        if let Some(w) = e.wakes.first().filter(|w| w.due_at_ms > now) {
            let past = w.due_at_ms - now + self.rng.random_range(0..5_000);
            self.clock.advance(past);
        }
        Ok(())
    }

    /// End the turn `g` holds on a free wait (input, a due time, its jobs),
    /// or otherwise: a wake that fell due while the turn ran is kept, and the
    /// end that would park the execution queues it instead (DD8).
    pub(super) fn end_a_turn(&mut self, g: TurnGuard, end: TurnEnd) -> Result<Execution> {
        let id = g.execution_id.clone();
        let before = self.kernel.execution(&id)?.unwrap();
        let end = self.task_end(&before, end);
        let due = wake_due(&before, self.now()) && before.stopped.is_none();
        let parks = matches!(
            end,
            TurnEnd::Wait {
                wake: Wake::Input | Wake::DueAt { .. } | Wake::Actions { .. }
            }
        );
        let e = self.end_turn_posting(g, end)?;
        if due && parks && !e.state.is_terminal() {
            if e.state != ExecState::Queued || !e.wakes.iter().any(|w| w.due_at_ms <= self.now()) {
                bail!(
                    "{id}'s turn ended with a wake due, and left it {:?} {:?} holding {:?}",
                    e.state,
                    e.wake,
                    e.wakes
                );
            }
            self.rep.sim2.busy_wakes += 1;
        }
        Ok(e)
    }

    /// After the heartbeat's due scan (`fire_due`), no open execution is
    /// left parked with a due time come, or free with a wake of its own due
    /// or a report's wake (W1): each was queued. A stop's report wake waits
    /// only for this scan.
    pub(super) fn check_due_scan(&mut self) -> Result<()> {
        let now = self.now();
        for e in self.kernel.open_executions()? {
            if due_now(&e, now) {
                bail!(
                    "the due scan at {now} left {} {:?} on {:?}, with wakes {:?} and report wakes {:?}",
                    e.id,
                    e.state,
                    e.wake,
                    e.wakes,
                    e.report_wakes
                );
            }
        }
        self.s2.stopped_since_scan.clear();
        Ok(())
    }
}
