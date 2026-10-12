//! An input's turn is admitted in the frame that writes its input
//! (theseus-2uby): the turn holds its execution from its start
//! (`Kernel::hold_turn`, nothing written), and the input's frame writes the
//! admission (the wake and `running`), the turn's waiting rows
//! (`turn.started`), and the input's node, in that order: what were two
//! frames, two syncs before the model's first byte, is one.
//!
//! A turn that must write before its input (a result to absorb, a report, a
//! due wake, a call to resume: `catch_up`) is admitted first, alone, as it
//! was before. So is an MCP prompt's input, which writes its own frame. A
//! turn that ends before it was admitted (a fault before its input) is
//! admitted in its end's frame. A `/cancel` that landed since the hold
//! refuses the admission under the frame's lock (`admit_held`): the input is
//! not written, and the turn fails `execution_cancelled`. A `/stop` since
//! the hold marks the held turn in the kernel (`turn_running`), and one that
//! landed between the input's arrival and the hold (`stopped_since`,
//! theseus-hmwv) is marked in the input's frame: either way its first step
//! plans nothing.

use super::*;

impl TurnRunner {
    /// Inside `k`'s frame: admit the turn `t` holds, and mark the `/stop`
    /// that landed since its input arrived. Writes nothing for a turn
    /// admitted already. A stop that cannot be marked takes back only itself.
    pub(super) fn admit_in(&self, k: &Kernel, t: &Turn<'_>) -> Result<bool> {
        let id = t.tc.execution_id;
        let wrote = k.admit_held(t.tc.guard)?;
        if wrote {
            if let Some(by) = self.stopped_since(id, t.arrived) {
                if let Err(e) = k.frame(&[id], |k| k.stop_execution(id, &by)) {
                    tracing::warn!(execution_id = %id, error = %format!("{e:#}"), "a stop before admission could not mark the turn");
                }
            }
        }
        Ok(wrote)
    }

    /// The held turn's admission in a frame of its own, with the turn's
    /// waiting rows after it: for a turn that writes before its input.
    pub(super) fn admit_alone(&self, t: &Turn<'_>) -> Result<()> {
        if t.tc.kernel.admitted(t.tc.guard)? {
            return Ok(());
        }
        self.admit_with(t, Vec::new())
    }

    /// Before `catch_up`: a held turn whose execution has anything for it to
    /// take or resume is admitted first, since what it writes may need the
    /// turn (a resumed call's plan). A plain turn's has nothing, and its
    /// admission waits for its input's frame; one whose only news is its
    /// tasks' reports is admitted in the frame that takes them
    /// (`read_reports`).
    pub(super) fn admit_before_catch_up(&self, t: &Turn<'_>) -> Result<()> {
        if t.tc.kernel.admitted(t.tc.guard)? {
            return Ok(());
        }
        let Some(e) = t.tc.kernel.execution(t.tc.execution_id)? else {
            return Ok(());
        };
        let now = t.tc.kernel.now_ms();
        let quiet = e.queued_results.is_empty()
            && e.outstanding.is_empty()
            && e.budget.question.is_none()
            && !e.resume_pending
            && e.wakes.iter().all(|w| w.due_at_ms > now);
        // Reports alone: their frame carries the admission.
        if quiet {
            return Ok(());
        }
        self.admit_alone(t)
    }

    /// The input's frame: the admission, the turn's waiting rows, and
    /// `records` (the input's node, and its session's record when the turn
    /// moved it), one frame. A turn admitted already writes the rest alone,
    /// as before. A refused admission (a cancel landed) writes nothing.
    pub(super) fn admit_with(&self, t: &Turn<'_>, records: Vec<NewRecord>) -> Result<()> {
        let id = t.tc.execution_id;
        #[cfg(test)]
        before_admission(t.tc.session_id);
        let waiting = t.tc.store.take_waiting();
        let staged = waiting.clone();
        let wrote = t.tc.kernel.frame(&[id], |k| {
            self.admit_in(k, t)?;
            k.stage(&staged)?;
            k.stage(&records)?;
            Ok(())
        });
        if wrote.is_err() {
            // Not written: the rows wait for the turn's next frame.
            for r in waiting {
                t.tc.store.defer(r)?;
            }
        }
        wrote
    }

    /// The turn `t` holds could not be admitted, because the execution ended
    /// since the hold (a cancel): the turn fails as an input refused at its
    /// admission does, its input unwritten. Any other error is the caller's.
    pub(super) fn not_admitted(
        t: &mut Turn<'_>,
        author: &str,
        e: anyhow::Error,
    ) -> Result<Failure> {
        let Some(KernelError::NotRunnable { state, .. }) = e.downcast_ref::<KernelError>() else {
            return Err(e);
        };
        let state = *state;
        t.record(&fact::turn::TurnNotRunnable { author, state });
        Ok(Failure {
            class: format!("execution_{state}"),
            transient: false,
            usage_unknown: false,
            reason: format!("execution_{state}: its input was not written"),
            source: e,
        })
    }
}

/// What a test runs on a turn's thread just before its input's frame.
#[cfg(test)]
type Hook = Box<dyn FnOnce() + Send>;

/// A test's seam (theseus-2uby): what runs on the turn's thread just before
/// its input's frame, by session, once (a cancel or a stop landing while the
/// turn is held).
#[cfg(test)]
static BEFORE_ADMISSION: std::sync::Mutex<Vec<(String, Hook)>> = std::sync::Mutex::new(Vec::new());

/// Run `f` just before the input's frame of the next turn of `session_id`.
#[cfg(test)]
pub(crate) fn admit_hook(session_id: &str, f: Hook) {
    BEFORE_ADMISSION
        .lock()
        .unwrap()
        .push((session_id.to_string(), f));
}

#[cfg(test)]
fn before_admission(session_id: &str) {
    let hook = {
        let mut hooks = BEFORE_ADMISSION.lock().unwrap();
        hooks
            .iter()
            .position(|(s, _)| s == session_id)
            .map(|i| hooks.remove(i).1)
    };
    if let Some(f) = hook {
        f();
    }
}
