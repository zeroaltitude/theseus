//! The sweep of unsettled drafts (theseus-b0ozi, MED-4). A draft is settled
//! when its `task.draft` row is written. A daemon that died between a step's
//! end frame and the next step, or a cancel that cascaded from the parent so
//! no review could open, leaves a record that is not settled and a tree on
//! disk. The sweep reads the `draft.` records, and for each one not settled
//! whose latest session (the first pass, the review, or the draft itself) has
//! ended, runs the step that session's end owes (`draft_ended`, which is
//! idempotent and one at a time per draft). A session still running is left
//! alone, except a first pass past its bound, which is cancelled.
//!
//! It runs once after serving (`Core::settle_drafts_after_serving`) and after
//! each cancel, for the drafts of the conversation that was cancelled.

use std::sync::Arc;

use theseus_store::{kinds, Store as _};

use super::Draft;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::turn::TurnRunner;

const PREFIX: &str = "draft.";

impl Core {
    /// Settle the drafts a crash left, once, after serving: never on the
    /// start path, and the core held by `Weak` meanwhile.
    pub fn settle_drafts_after_serving(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(core) = core.upgrade() {
                core.runner.sweep_drafts(None).await;
            }
        });
    }
}

impl TurnRunner {
    /// A cancel of `session_id`'s execution: its own end, and then the drafts
    /// of the conversation it may have left without a review.
    pub async fn draft_cancelled(&self, session_id: &str) {
        self.draft_ended(session_id).await;
        self.sweep_drafts(Some(session_id)).await;
    }

    /// Run the owed step of each draft not settled whose latest session has
    /// ended; with `parent`, those of that conversation alone.
    pub async fn sweep_drafts(&self, parent: Option<&str>) {
        let rows = match self.store.inner().latest_with_prefix(kinds::META, PREFIX) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the unsettled drafts were not read");
                return;
            }
        };
        for r in rows {
            let Some(sid) = r.key.as_deref().and_then(|k| k.strip_prefix(PREFIX)) else {
                continue;
            };
            let Ok(d) = r.decode::<Draft>() else {
                continue;
            };
            if d.settled || parent.is_some_and(|p| p != d.parent_session) {
                continue;
            }
            let latest = match (
                &d.review,
                d.first_pass.as_ref().and_then(|f| f.task.as_ref()),
            ) {
                (Some(review), _) => review.as_str(),
                (None, Some(task)) => task.as_str(),
                (None, None) => sid,
            };
            if !self.ended(latest) {
                if latest != sid && d.review.is_none() {
                    self.expire_first_pass(sid).await;
                }
                continue;
            }
            self.draft_ended(latest).await;
        }
    }

    /// Whether the session `id`'s execution has ended.
    fn ended(&self, id: &str) -> bool {
        let rec = self.store.get_session::<SessionRecord>(id).ok().flatten();
        let exec = rec.and_then(|r| r.execution_id);
        matches!(
            exec.map(|e| self.kernel.execution(&e)),
            Some(Ok(Some(e))) if e.state.is_terminal()
        )
    }
}
