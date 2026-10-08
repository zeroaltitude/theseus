//! The daemon's day ceiling refused a loop's provider call (theseus-kp20):
//! the kernel planned nothing and wrote nothing (`KernelError::DayCeiling`),
//! so the provider is never called. The turn ends failed, class
//! `daily_ceiling`, which is lasting and settled nothing: its run parks the
//! execution at once (`Failing::after`), so the driver retries nothing, and
//! its notice carries the words to the session's place (a task's failure
//! ends it, and its report says why). The day's first refusal writes its
//! `spend.ceiling` row and the owner's post (`TurnRunner::day_refused`).

use super::*;

/// The failure's class.
pub const DAY_CEILING_CLASS: &str = "daily_ceiling";

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
        Failure {
            class: DAY_CEILING_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: DAY_CEILING_CLASS.into(),
            source: anyhow::anyhow!("{r}"),
        }
    }
}
