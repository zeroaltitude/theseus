//! A stale in-progress item is settled (issue 6, condition 8).
//!
//! An item that last failed days earlier still showed as in progress with no build behind it:
//! its record said "in progress", and nothing in the pipeline's queues held it, so nothing
//! would ever change it. A tick now settles such an item as failed, with one row and its
//! reason in its evidence, and closes whatever trees it left. It is not a try (no build was
//! judged), and the owner's `theseus self retry` brings it back as for any failed item.

use anyhow::Result;
use theseus_protocol::tasks::TaskState;

use super::queues::Queues;
use super::{evidence, Moved, Transition, Write};
use crate::fact::rsi_pipeline::Ended;
use crate::rpc::Core;
use crate::rsi::tree::Place;

/// The words the evidence and the row carry.
pub(super) const STALE: &str = "it was in progress with no build behind it";

impl Core {
    /// Settle every stale in-progress item, once.
    pub(super) async fn pipeline_stale(
        &self,
        place: &Place,
        q: &Queues,
        w: &mut Write,
    ) -> Result<()> {
        let now = theseus_protocol::now_unix_ms();
        for e in &q.stale {
            let Some(mut t) = crate::task_graph::get(&self.store, &e.item)? else {
                continue;
            };
            if t.state != TaskState::InProgress {
                continue;
            }
            t.state = TaskState::Failed;
            t.version += 1;
            t.updated_at_ms = now;
            t.evidence
                .push(evidence("settled:stale".into(), STALE.into(), now));
            w.task(q, &t)?;
            let m = Moved::new(&e.item, format!("{} was settled as failed", e.item))
                .with("reason", "stale")
                .why(STALE);
            w.say(
                format!("stale:{}:{}", e.item, t.version),
                Transition::Ended(Ended(m)),
            )?;
            self.close_item_trees(place, e, "stale", w).await?;
        }
        Ok(())
    }
}
