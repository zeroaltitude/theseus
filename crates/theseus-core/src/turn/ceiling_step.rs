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

/// The failure's class when the runaway line stopped the day (theseus-9e02).
pub const DAY_RUNAWAY_CLASS: &str = "daily_runaway";

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
        let class = match r.runaway {
            true => DAY_RUNAWAY_CLASS,
            false => DAY_CEILING_CLASS,
        };
        let line = if r.runaway { "runaway line" } else { "ceiling" };
        t.record(&fact::turn::LoopCut { decision: class });
        // A shared place hears that the ceiling stopped the call and when the
        // day turns, never the owner's totals (the place rule): those go to
        // the owner's DM alone, with the day's row.
        let words = match t.tc.class {
            crate::places::PlaceClass::Shared => format!(
                "the daemon's daily spend {line} was reached, so the call was not made, and \
                 none will be until the day turns at {} local time; this turn ends, it does \
                 not wait",
                r.turns_at
            ),
            _ => r.to_string(),
        };
        let note = Node::relayed(
            t.tc.session_id,
            Some(t.tc.turn_id),
            crate::node::Origin::Harness,
            NOTE_AUTHOR,
            &format!("⛔ day {line}: this turn's model call was not made: {words}."),
        );
        match note.record().and_then(|rec| t.tc.store.append(&[rec])) {
            Ok(_) => t.tc.node_written(&note),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "day ceiling: the session's note was not written");
            }
        }
        Failure {
            class: class.into(),
            transient: false,
            usage_unknown: false,
            reason: class.into(),
            source: anyhow::anyhow!("{words}"),
        }
    }
}

impl TurnRunner {
    /// Under notify, the day's warning (theseus-9e02): when today's spend is
    /// at the ceiling or a step past it and nobody was told of that level, one
    /// post to the owner's DM (when one is bound), one `spend.day_warning`
    /// row, and one narrative line, all riding the turn's next frame. A check
    /// that finds nothing new costs one comparison under one small lock.
    pub(super) fn day_warned(&self, t: &mut Turn<'_>) -> Result<()> {
        let c = t.tc.kernel.day_ceiling();
        let Some(w) = c.take_warning(c.now()) else {
            return Ok(());
        };
        let text =
            crate::day_ceiling::warning_text(&w, &super::budget_step::stop_how(t.tc.session_id));
        let mut posted = false;
        if self.place_rule.owner_dm_bound() {
            let body = json!({"kind": "spend_ceiling", "text": text});
            let (post, records) = self.outbox.stage_to_operator(Some(t.tc.session_id), body)?;
            for r in records {
                t.tc.store.defer(r)?;
            }
            t.tc.posts.lock().unwrap().push(post);
            posted = true;
        }
        tracing::warn!(day = %w.day, level = w.level, spent_usd = theseus_kernel::micros_to_usd(w.spent),
            ceiling_usd = theseus_kernel::micros_to_usd(w.limit), "day ceiling: the day's spend passed it, and calls go on");
        t.record(&fact::limits::DayCeilingWarned {
            warning: &w,
            posted,
            text: &text,
        });
        Ok(())
    }
}
