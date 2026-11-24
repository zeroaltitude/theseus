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
//!   freed here. A call already settled or held, or one a cancel was told to stop, is left as it
//!   is: the cancel's own step holds it the same way.
//! - **Settled** ([`Owed::settle`]): by the call's own completion, once; the drop then does nothing.
//!   An unknown one is left to a cancel that was told to stop the call and has not yet settled it.
//!
//! Each settlement is one step (`land`): in one frame on the call's execution, the call is read,
//! the decision taken, and the completion accepted through that frame's view. A cancel marks the
//! call under the same lock, so it marks it either before the read, and the owner leaves it to the
//! cancel, or after the write, and finds it no longer open. The day never counts it twice.
//!
//! A drop settles inline, on the dropping thread, before it returns. It spawns no thread, and waits
//! only as any frame does (for its execution's lock, for the store's write). The tools keep their
//! owner in the tool's task, outside any kernel frame, so that is every drop today. An owner must not be dropped inside a kernel frame: that thread already holds
//! a lock, and the settlement's frame would be a lock inside a lock (the kernel panics on it). Such
//! a drop writes nothing: it logs one warning and returns at once, and the call stays dispatched
//! until the reconcile holds it as unknown, past its deadline or at the next start. A store error
//! from the settlement's frame is logged the same way, and the reconcile settles what is left.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, Completion, Kernel, Outcome};
use theseus_store::NewRecord;

/// The owner of one reserved provider call: it settles the call once, by its completion or on drop.
/// It must not be dropped inside a kernel frame: such a drop leaves the call to the reconcile.
pub struct Owed {
    kernel: Arc<Kernel>,
    correlation_id: String,
    execution_id: String,
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
            execution_id: action.execution_id.clone(),
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
        land(&self.kernel, &self.execution_id, c, extra, by_its_own).map(|_| ())
    }

    /// The completion a drop settles with: at zero before the send, unknown after it.
    fn dropped(&self, sent: bool) -> Completion {
        let now = self.kernel.now_ms();
        let (outcome, cost, dropped) = match sent {
            false => (Outcome::Failed, Some(0), "before_send"),
            true => (Outcome::Unknown, None, "after_send"),
        };
        let mut detail = self.detail.clone();
        if let Value::Object(d) = &mut detail {
            d.insert("dropped".into(), json!(dropped));
        }
        Completion {
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
        }
    }
}

impl Drop for Owed {
    fn drop(&mut self) {
        if *self.settled.get_mut() {
            return;
        }
        if theseus_kernel::this_thread_holds_a_lock() {
            tracing::warn!(correlation_id = %self.correlation_id,
                "a provider call dropped inside a kernel frame is not settled here; the reconcile settles it");
            return;
        }
        let sent = *self.sent.get_mut();
        let c = self.dropped(sent);
        if let Err(e) = land(&self.kernel, &self.execution_id, &c, Vec::new(), by_a_drop) {
            tracing::warn!(correlation_id = %self.correlation_id, error = %format!("{e:#}"),
                "a dropped provider call was not settled here; the reconcile settles it");
        }
    }
}

/// Settle the call by `c` in one step: in one frame on its execution, read the call, ask `lands`,
/// and accept `c` through the frame's view. `Ok(false)`: it did not land, and nothing is written.
fn land(
    kernel: &Kernel,
    execution_id: &str,
    c: &Completion,
    extra: Vec<NewRecord>,
    lands: fn(&Action, &Completion) -> bool,
) -> Result<bool> {
    kernel.frame(&[execution_id], |k| match k.action(&c.correlation_id)? {
        Some(a) if lands(&a, c) => k.accept_completion_with(c, extra).map(|_| true),
        _ => Ok(false),
    })
}

/// A cancel was told to stop `a` and its own step has not settled it: that step holds an unknown
/// cost, so an unknown landing first would have the day count the reservation twice.
fn the_cancels(a: &Action) -> bool {
    a.state == ActionState::Dispatched && a.cancel.is_some()
}

/// The call's own completion lands, but an unknown one on a call [`the_cancels`]. A known cost
/// lands even then: the call is settled by it, and the cancel's step leaves a settled call alone.
fn by_its_own(a: &Action, c: &Completion) -> bool {
    c.outcome != Outcome::Unknown || !the_cancels(a)
}

/// A drop's zero (never sent) lands as the kernel takes any known cost. Its unknown lands only on
/// a call still open that no cancel was told to stop: one settled, held or marked is left as it is.
fn by_a_drop(a: &Action, c: &Completion) -> bool {
    c.outcome != Outcome::Unknown || (a.state == ActionState::Dispatched && a.cancel.is_none())
}

#[cfg(test)]
#[path = "tests_owed_call.rs"]
mod tests;
