//! Where a task reports, read when it reports (theseus-jid7t.2). A turn holds
//! the task's record as it read it when it began, and `task.rehome` may move
//! the report while the turn runs, so the frame that ends the task reads the
//! target again: the report goes where the owner last said, never to a place
//! the turn began with.

use crate::session::{SessionRecord, TaskOf};
use crate::store::Store;

impl TaskOf {
    /// [`TaskOf::target_for`] over the record as stored now, falling back to
    /// this copy when the record cannot be read.
    pub(crate) fn target_for_now(
        &self,
        store: &Store,
        session_id: &str,
        report: &crate::task::Report,
    ) -> Option<String> {
        let held = store
            .get_session::<SessionRecord>(session_id)
            .ok()
            .flatten()
            .and_then(|r| r.task);
        held.as_ref()
            .unwrap_or(self)
            .target_for(store, report)
            .cloned()
    }
}
