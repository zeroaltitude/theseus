//! An earlier process's in-process calls (theseus-m9iy).
//!
//! A provider call runs in the daemon's own process: it has no wrapper, no
//! spool file, and no evidence a reconciler can read. When the daemon dies
//! with one in flight, the process that ran it is gone, so after the restart
//! the call can never complete; it used to stay `dispatched` until its
//! deadline (600 s by default), when the heartbeat marked it unknown
//! (`overdue_no_evidence`).
//!
//! Startup's reconcile already reads every dispatched action. In that scan
//! each in-process call is noted ([`Evidence::in_process`]; at startup every
//! one is an earlier process's), and nothing is written. Once the socket
//! serves, the driver's first tick marks them all `outcome_unknown`, with
//! [`EARLIER_PROCESS`] as the reason, in one frame, as due wakes wait for
//! that tick (DD8); the heartbeat's reconcile does it too, if the driver has
//! not. A job keeps its evidence (its wrapper, its spool), so it is left as
//! it was.

use anyhow::Result;

use crate::kernel::{Evidence, Kernel, KernelError};
use crate::types::{Action, CorrelationId};

/// The reason an earlier process's in-process call is marked unknown: its
/// completion's producer is `reconciler:in_process_before_restart`.
pub const EARLIER_PROCESS: &str = "in_process_before_restart";

impl Kernel {
    fn earlier_calls(&self) -> std::sync::MutexGuard<'_, Vec<CorrelationId>> {
        self.earlier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Startup's scan: note `a`, a dispatched action not yet overdue, when it
    /// ran in this kernel's process, which at startup was an earlier one.
    pub(crate) fn note_earlier(&self, a: &Action, evidence: &dyn Evidence) {
        if evidence.in_process(a) {
            self.earlier_calls().push(a.correlation_id.clone());
        }
    }

    /// Mark every in-process call startup found `outcome_unknown`, in one
    /// frame, once: the driver's first tick after serving, or the heartbeat.
    /// A call settled since is left as it is. The calls it marked.
    pub fn mark_earlier_calls_unknown(&self) -> Result<Vec<CorrelationId>> {
        let calls = std::mem::take(&mut *self.earlier_calls());
        if calls.is_empty() {
            return Ok(calls);
        }
        let mut ids: Vec<String> = Vec::new();
        for c in &calls {
            if let Some(a) = self.action(c)? {
                if !ids.contains(&a.execution_id) {
                    ids.push(a.execution_id);
                }
            }
        }
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.frame(&ids, |k| {
            let mut marked = Vec::new();
            for c in &calls {
                match k.mark_unknown(c, EARLIER_PROCESS) {
                    Ok(_) => marked.push(c.clone()),
                    Err(e)
                        if matches!(
                            e.downcast_ref::<KernelError>(),
                            Some(KernelError::ActionState { .. } | KernelError::UnknownAction(_))
                        ) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(marked)
        })
    }
}
