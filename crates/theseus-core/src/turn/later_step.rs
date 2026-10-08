//! What a turn left for later, on a daemon spawned for one run
//! (theseus-mqxk, `one_shot.rs`): the result's `later`, which `theseus
//! --spawn ask` follows while its bound lasts and names when it ends.
//!
//! The jobs are read twice: before the turn takes its late results, and as
//! it parks. A job that settles between the two is in neither the late
//! results the turn took nor the jobs it parks on, and the frame that ends
//! the turn wakes it for that result (theseus-6qwr): so the count falling
//! there says a turn is queued, as a late result the turn took does. Any
//! other daemon reads nothing here, and its results carry no `later`.

use theseus_protocol::later::{Later, LaterWake};

use super::*;

impl TurnRunner {
    /// On a one-run daemon, the jobs outstanding before the turn takes its
    /// late results; None on any other.
    pub(super) fn jobs_before(&self, t: &Turn<'_>) -> Option<usize> {
        self.outbox.one_shot.follow_ms()?;
        self.jobs_outstanding(t.tc.execution_id, &t.background).ok()
    }

    /// The turn's `later`, from the kernel as the turn parks: `before` is
    /// `jobs_before`'s count, and `again` whether the turn takes another
    /// for a late result it read. None unless `before` was read, or when
    /// the turn left nothing.
    pub(super) fn later_of(
        &self,
        t: &Turn<'_>,
        before: Option<usize>,
        again: bool,
    ) -> Option<Later> {
        let before = before?;
        let jobs = self
            .jobs_outstanding(t.tc.execution_id, &t.background)
            .unwrap_or(before);
        let wakes = self
            .kernel
            .execution(t.tc.execution_id)
            .ok()
            .flatten()
            .map(|e| e.wakes)
            .unwrap_or_default();
        let mut wakes: Vec<LaterWake> = wakes
            .into_iter()
            .map(|w| LaterWake {
                wake_id: w.id,
                due_at_ms: w.due_at_ms,
                note: w.note,
            })
            .collect();
        wakes.sort_by_key(|w| w.due_at_ms);
        let later = Later {
            jobs: jobs as u32,
            queued: again || jobs < before,
            wakes,
        };
        (!later.is_empty()).then_some(later)
    }

    /// The execution's calls dispatched and not settled that are jobs, as
    /// `park` counts them: the turn's background jobs, and any call but a
    /// provider's.
    fn jobs_outstanding(&self, exec_id: &str, background: &[String]) -> anyhow::Result<usize> {
        let outstanding = self
            .kernel
            .execution(exec_id)?
            .map(|e| e.outstanding)
            .unwrap_or_default();
        Ok(outstanding
            .iter()
            .filter(|c| {
                background.contains(c)
                    || self
                        .kernel
                        .action(c)
                        .ok()
                        .flatten()
                        .is_some_and(|a| a.tool != PROVIDER_TOOL && a.tool != BUDGET_TOOL)
            })
            .count())
    }
}
