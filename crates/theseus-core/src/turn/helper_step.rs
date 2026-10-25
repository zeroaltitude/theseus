//! A helper at the end of its carve (theseus-z9mze.3): `task.fan_out`'s helpers
//! each have an equal share of the money, and a helper that has spent it is
//! done, not asking.
//!
//! - **The note.** At 75 % of its carve a helper is told once, in a harness node
//!   in its tail, to write its report now: its findings so far, then what it did
//!   not reach. It is not the progress check's steer (`progress_step.rs`), which
//!   asks a different thing.
//! - **The end.** At its carve the turn does not ask the budget question, which
//!   nobody answers in a headless run and which would park the helper until the
//!   wait timed out. It ends instead, with the stop reason `budget`
//!   (`at_carve`), and `task_end` completes it with the reason `budget`: the
//!   task reports with the state `budget` and its findings so far. A plain task
//!   keeps its question.

use super::*;

/// The author of the note, and so how it is told it was written.
pub const NOTE_AUTHOR: &str = "harness:helper";
/// The share of its carve at which the note is written.
pub const NOTE_AT: (u64, u64) = (3, 4);
/// What the note says.
pub const NOTE: &str = "[The harness: you have used most of your share of the budget. Write your \
    report now: your findings so far, then what you did not reach.]";

/// Whether `execution_id` is a helper: opened by `task.fan_out`.
pub(super) fn is_helper_exec(kernel: &theseus_kernel::Kernel, execution_id: &str) -> bool {
    kernel
        .execution(execution_id)
        .ok()
        .flatten()
        .is_some_and(|e| e.helper_of().is_some())
}

/// A helper's call did not fit its carve: the turn ends here with the stop reason
/// `budget`. False for every other execution, which asks its question.
pub(super) fn at_carve(t: &mut Turn<'_>) -> bool {
    let helper = is_helper_exec(t.tc.kernel, t.tc.execution_id);
    if helper {
        t.stop_reason = "budget".into();
    }
    helper
}

impl TurnRunner {
    /// Before a loop's compile: the one note a helper gets at 75 % of its carve.
    pub(super) fn helper_note(t: &mut Turn<'_>) -> Result<()> {
        let Ok(Some(e)) = t.tc.kernel.execution(t.tc.execution_id) else {
            return Ok(());
        };
        if e.helper_of().is_none() || e.budget.limit_micros == 0 {
            return Ok(());
        }
        let (num, den) = NOTE_AT;
        if e.budget.spent_micros.saturating_mul(den) < e.budget.limit_micros.saturating_mul(num) {
            return Ok(());
        }
        let said =
            t.tc.store
                .transcript(t.tc.session_id)?
                .iter()
                .any(|(_, n)| n.author.as_deref() == Some(NOTE_AUTHOR));
        if said {
            return Ok(());
        }
        let node = Node::relayed(
            t.tc.session_id,
            Some(t.tc.turn_id),
            crate::node::Origin::Harness,
            NOTE_AUTHOR,
            NOTE,
        );
        t.tc.store.append(&[node.record()?])?;
        t.tc.node_written(&node);
        Ok(())
    }
}
