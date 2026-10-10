//! kernel-sim's held input turns (theseus-2uby): half of an input's turns
//! hold the execution first (`hold_turn`, nothing written) and write the
//! admission later (`admit_held`), as the core's input frame does, with a
//! crash, a cancel, or a stop now and then between the two. A crash leaves
//! the execution as it was; a cancel refuses the admission and writes
//! nothing; a stop marks the held turn, the admission keeps the mark, and the
//! turn ends parked on input having planned nothing.

use anyhow::{bail, Result};
use theseus_kernel::*;

use super::World;

impl World {
    /// An input's turn on `id`: admitted in one frame (`admit_input`), or held
    /// and then admitted. None when it took no turn: admission must wait, a
    /// crash or a cancel came between the hold and the admission, or a stop
    /// did and its turn ended at once.
    pub(super) fn admit_by_input(&mut self, id: &str) -> Result<Option<TurnGuard>> {
        let busy = |err: &anyhow::Error| {
            matches!(
                err.downcast_ref::<KernelError>(),
                Some(KernelError::AdmissionFull { .. }) | Some(KernelError::TurnHeld { .. })
            )
        };
        if !self.chance(0.5) {
            return match self.kernel.admit_input(id) {
                Ok(g) => Ok(Some(g)),
                Err(err) if busy(&err) => Ok(None),
                Err(err) => Err(err),
            };
        }
        let before = self.kernel.store().last_position();
        let g = match self.kernel.hold_turn(id) {
            Ok(g) => g,
            Err(err) if busy(&err) => return Ok(None),
            Err(err) => return Err(err),
        };
        if self.kernel.store().last_position() != before {
            bail!("the hold of a turn on {id} wrote");
        }
        self.rep.sim2.held_turns += 1;
        if self.maybe_crash("after hold")? {
            return Ok(None);
        }
        let between = if self.chance(0.1) {
            self.kernel.cancel_execution(id, "operator")?;
            self.rep.sim2.held_cancelled += 1;
            "cancel"
        } else if self.chance(0.1)
            && self
                .kernel
                .execution(id)?
                .is_some_and(|e| e.kind == SessionKind::Conversation && e.parent.is_none())
        {
            let stop = self.kernel.stop_execution(id, "operator")?;
            if !stop.as_ref().is_some_and(|s| s.turn_running) {
                bail!("a stop of {id} while a turn held it did not count the turn: {stop:?}");
            }
            self.rep.sim2.held_stopped += 1;
            "stop"
        } else {
            ""
        };
        match self.kernel.admit_held(&g) {
            Ok(true) => {}
            Ok(false) => bail!("a held turn on {id} was admitted already"),
            Err(err) if between == "cancel" => {
                if !matches!(
                    err.downcast_ref::<KernelError>(),
                    Some(KernelError::NotRunnable {
                        state: "cancelled",
                        ..
                    })
                ) {
                    bail!("a cancelled held turn on {id} was refused otherwise: {err:#}");
                }
                if self
                    .kernel
                    .execution(id)?
                    .is_some_and(|e| e.turns >= g.turn)
                {
                    bail!("a cancelled held turn on {id} was counted");
                }
                return Ok(None);
            }
            Err(err) => return Err(err),
        }
        if between == "cancel" {
            bail!("a held turn on {id} was admitted after its cancel");
        }
        let e = self.kernel.execution(id)?.unwrap();
        if e.state != ExecState::Running || e.turns != g.turn {
            bail!(
                "a held turn's admission left {id} {:?} at turn {}",
                e.state,
                e.turns
            );
        }
        if between == "stop" {
            if e.stopped.is_none() {
                bail!("the admission of a held turn on {id} lost its stop's mark");
            }
            let e = self.end_turn_posting(g, TurnEnd::Wait { wake: Wake::Input })?;
            if !(e.state == ExecState::Waiting && e.stopped.is_none()) {
                bail!("a held turn's stopped end left {id} {:?}", e.state);
            }
            return Ok(None);
        }
        self.rep.sim2.held_admitted += 1;
        Ok(Some(g))
    }
}
