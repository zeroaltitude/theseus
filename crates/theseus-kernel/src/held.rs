//! A turn held before its admission is written (theseus-2uby). An input's
//! turn takes its slot in this process first (`hold_turn`: the checks
//! `admit` makes, nothing written), and its admission (`admit_held`: the
//! input's wake and `running`) is written in the frame that writes its input,
//! so a plain turn starts with one sync, not two.
//!
//! Between the two the execution reads as it was (waiting, or queued), and
//! the slot keeps every other turn off it: the driver's continuations find
//! it held, and a cancel's sweep waits for the turn's end, as it does for a
//! running turn. A `/cancel` that lands meanwhile is seen when the frame
//! takes the lock: `admit_held` re-reads the execution there, refuses a
//! cancelled one (`NotRunnable`), and the frame writes nothing. A cancel and
//! a `/stop` count a held turn as running (`turn_running`): the cancel's
//! sweep waits for the turn's end, and the stop marks the turn, as it marks a
//! running one, and leaves the execution where it was, so the admission keeps
//! the mark and the turn's first plan is refused. A stop that parked it
//! instead would be undone by the admission's wake.
//!
//! No transition that needs the turn (`require_turn`: a plan, a budget
//! question, a task, a wake) may run before `admit_held`: the caller admits
//! first wherever its turn might reach one before its input's frame.

use crate::kernel::{Kernel, KernelError, TurnGuard};
use crate::types::ExecState;
use anyhow::Result;

impl Kernel {
    /// Hold a turn on `execution_id` for new input, writing nothing: it must
    /// be queued, waiting or blocked (input wakes it), no turn may be held on
    /// it, and the ceiling must have room. Refused as `admit_input` refuses:
    /// `NotRunnable` (`running` while a turn of another process's view runs
    /// it), `TurnHeld`, or `AdmissionFull`.
    pub fn hold_turn(&self, execution_id: &str) -> Result<TurnGuard> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state.is_terminal() || e.state == ExecState::Running {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        self.hold(&e)
    }

    /// Write the admission of the turn `guard` holds, as `admit_input` writes
    /// it: the input's wake (when the execution waits), then `running`, each
    /// with its record and row, in one frame, or in the caller's. Returns
    /// whether it wrote: a turn admitted already (by `admit`, `admit_input`,
    /// or an earlier call) writes nothing. The execution is read again here,
    /// under the frame's lock: a cancel that landed since the hold refuses it
    /// (`NotRunnable`), and the frame writes nothing.
    pub fn admit_held(&self, guard: &TurnGuard) -> Result<bool> {
        let id = guard.execution_id.as_str();
        self.frame(&[id], |k| {
            if k.admitted(guard)? {
                return Ok(false);
            }
            k.wake_input(id)?;
            let _w = k.lock(&[id]);
            let e = k
                .execution(id)?
                .ok_or_else(|| KernelError::UnknownExecution(id.into()))?;
            if e.state != ExecState::Queued {
                return Err(KernelError::NotRunnable {
                    id: e.id.clone(),
                    state: e.state.as_str(),
                }
                .into());
            }
            k.write_running(e, guard)?;
            Ok(true)
        })
    }

    /// Whether the turn `guard` holds is written as running: the execution
    /// has counted it.
    pub fn admitted(&self, guard: &TurnGuard) -> Result<bool> {
        Ok(self
            .execution(&guard.execution_id)?
            .is_some_and(|e| e.turns >= guard.turn))
    }
}
