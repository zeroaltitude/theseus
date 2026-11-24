//! The owner of a provider call a tool plans in its turn and makes in its own task (`task.ask`,
//! `profile.ask`): the one value that settles the call's reservation, whatever ends the task.
//!
//! The call is reserved and dispatched in the turn, before the tool's task is spawned; a stop or a
//! deadline can abort that task at any await, and an aborted task never reaches its settle. So the
//! owner settles on drop, through the kernel's own completion calls:
//!
//! - **Dropped before the send** (its task never polled past [`Owed::sending`]): nothing can have
//!   left, so the call is settled as failed at zero, and its reservation released.
//! - **Dropped once the send may have begun**: settled as unknown at once. The kernel holds the
//!   whole reservation, as it holds every unknown cost: money that may have been spent is never
//!   freed here. A call a cancel was told to stop is left to the cancel, which holds it the same way.
//! - **Settled** ([`Owed::settle`]): by the call's own completion, once; the drop then does nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Action, Completion, Kernel, Outcome};
use theseus_store::NewRecord;

/// The owner of one reserved provider call: it settles the call once, by its completion or on drop.
pub struct Owed {
    kernel: Arc<Kernel>,
    correlation_id: String,
    producer: String,
    detail: Value,
    sent: AtomicBool,
    settled: AtomicBool,
}

impl Owed {
    /// The owner of `action`, a provider call just reserved and dispatched. `producer` and
    /// `detail` are what a settlement on drop says, beside why it was dropped.
    pub fn new(kernel: Arc<Kernel>, action: &Action, producer: String, detail: Value) -> Self {
        Self {
            kernel,
            correlation_id: action.correlation_id.clone(),
            producer,
            detail,
            sent: AtomicBool::new(false),
            settled: AtomicBool::new(false),
        }
    }

    /// The send may begin from here: a drop now settles the call as unknown.
    pub fn sending(&self) {
        self.sent.store(true, Ordering::SeqCst);
    }

    /// Settle the call by its own completion `c`, with `extra` records in its frame. Once: a
    /// second call writes nothing, and the drop then does nothing.
    pub fn settle(&self, c: &Completion, extra: Vec<NewRecord>) -> Result<()> {
        if self.settled.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.kernel.accept_completion_with(c, extra).map(|_| ())
    }
}

impl Drop for Owed {
    fn drop(&mut self) {
        if *self.settled.get_mut() {
            return;
        }
        let sent = *self.sent.get_mut();
        if sent && self.cancelled() {
            return;
        }
        let now = self.kernel.now_ms();
        let (outcome, cost, dropped) = match sent {
            false => (Outcome::Failed, Some(0), "before_send"),
            true => (Outcome::Unknown, None, "after_send"),
        };
        let mut detail = self.detail.clone();
        if let Value::Object(d) = &mut detail {
            d.insert("dropped".into(), json!(dropped));
        }
        let c = Completion {
            correlation_id: self.correlation_id.clone(),
            outcome,
            result_ref: None,
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: self.producer.clone(),
            signature: None,
            cost_micros: cost,
            detail: Some(detail),
        };
        if let Err(e) = self.kernel.accept_completion_with(&c, Vec::new()) {
            tracing::warn!(correlation_id = %self.correlation_id, error = %format!("{e:#}"),
                "a dropped provider call was not settled; the reconcile settles it at its deadline");
        }
    }
}

impl Owed {
    /// A cancel was told to stop the call (its own step holds an unknown cost), or settled it.
    fn cancelled(&self) -> bool {
        matches!(self.kernel.action(&self.correlation_id), Ok(Some(a)) if a.cancel.is_some())
    }
}
