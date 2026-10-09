//! The daemon's day ceiling refused a loop's provider call (theseus-kp20):
//! the kernel planned nothing and wrote nothing (`KernelError::DayCeiling`),
//! so the provider is never called. The turn ends failed, class
//! `daily_ceiling`, which is lasting and settled nothing: its run parks the
//! execution at once (`Failing::after`), so the driver retries nothing, and
//! its notice carries the words to the session's place (a task's failure
//! ends it, and its report says why). The day's first refusal writes its
//! `spend.ceiling` row and the owner's post (`TurnRunner::day_refused`).
//! The session keeps a note of it, a node the model reads as the user's (as
//! a wake's or a task's report), so its next context says why its call did
//! not run.

use super::*;

/// The failure's class.
pub const DAY_CEILING_CLASS: &str = "daily_ceiling";

/// The note's author.
pub const NOTE_AUTHOR: &str = "harness:day_ceiling";

impl TurnRunner {
    pub(super) fn day_ceiling_failed(
        &self,
        t: &mut Turn<'_>,
        r: &theseus_kernel::Reached,
    ) -> Failure {
        let task =
            t.tc.kernel
                .execution(t.tc.execution_id)
                .ok()
                .flatten()
                .is_some_and(|e| e.kind == theseus_kernel::SessionKind::Task);
        let what = if task { "task" } else { "turn" };
        self.day_refused(r, what, Some(t.tc.session_id));
        t.record(&fact::turn::LoopCut {
            decision: DAY_CEILING_CLASS,
        });
        // A shared place hears that the ceiling stopped the call and when the
        // day turns, never the owner's totals (the place rule): those go to
        // the owner's DM alone, with the day's row.
        let words = match t.tc.class {
            crate::places::PlaceClass::Shared => format!(
                "the daemon's daily spend ceiling was reached, so the call was not made; \
                 the day turns at {} local time",
                r.turns_at
            ),
            _ => r.to_string(),
        };
        let note = Node::relayed(
            t.tc.session_id,
            Some(t.tc.turn_id),
            crate::node::Origin::Harness,
            NOTE_AUTHOR,
            &format!("⛔ day ceiling: this turn's model call was not made: {words}."),
        );
        match note.record().and_then(|rec| t.tc.store.append(&[rec])) {
            Ok(_) => t.tc.node_written(&note),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "day ceiling: the session's note was not written");
            }
        }
        Failure {
            class: DAY_CEILING_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: DAY_CEILING_CLASS.into(),
            source: anyhow::anyhow!("{words}"),
        }
    }
}
