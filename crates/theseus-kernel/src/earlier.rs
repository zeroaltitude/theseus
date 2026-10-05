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
//!
//! **Its money is booked, not held** (theseus-f3wr). The request reached the
//! provider before the crash and may have been charged, and no completion
//! can ever bring its real cost: the process that ran it is gone. So the
//! mark books the call's reservation as spent, an estimate that reads true
//! or slightly high (the reservation is the worst case), and a reset clears
//! it like any other spend. Held as unknown, it would shrink the session's
//! room for good, since a reset leaves held what it cannot free. The action
//! and its completion stay `OutcomeUnknown`: only the money is settled. The
//! action keeps `"cost_basis": "reservation"` in its `detail`, and so does
//! its `action.outcome_unknown` row, with the reservation as `cost_usd`.
//! Every other unknown mark (`mark_unknown`) holds, as before: there the call
//! may still run, or its cost may still arrive.

use anyhow::Result;
use serde_json::{json, Value};

use crate::kernel::{Evidence, Kernel, KernelError};
use crate::types::{micros_to_usd, Action, Budget, CorrelationId, Micros};

/// The reason an earlier process's in-process call is marked unknown: its
/// completion's producer is `reconciler:in_process_before_restart`.
pub const EARLIER_PROCESS: &str = "in_process_before_restart";

/// What a booked call's cost is: its reservation, as an estimate. The value
/// of `cost_basis` in the action's `detail` and in its row.
pub(crate) const COST_BASIS_RESERVATION: &str = "reservation";

/// The call was settled with its reservation booked as its cost.
pub(crate) fn booked(a: &Action) -> bool {
    a.detail
        .as_ref()
        .and_then(|d| d.get("cost_basis"))
        .and_then(Value::as_str)
        == Some(COST_BASIS_RESERVATION)
}

pub(crate) fn mark_booked(a: &mut Action) {
    a.detail = Some(json!({"cost_basis": COST_BASIS_RESERVATION}));
}

/// The `action.outcome_unknown` row of a booked call: its cost is the
/// reservation, as an estimate.
pub(crate) fn booked_row(data: &mut Value, a: &Action) {
    data["cost_usd"] = micros_to_usd(a.reserved_micros).into();
    data["cost_basis"] = COST_BASIS_RESERVATION.into();
}

/// Release a reservation into spend at its own amount: no cost is known,
/// and none will come.
pub(crate) fn book_reservation_in(b: &mut Budget, reservation_id: &str) {
    if let Some(&reserved) = b.reservations.get(reservation_id) {
        crate::kernel::settle_reservation_in(b, reservation_id, Some(reserved));
    }
}

/// An unknown call's outcome, learned later, with the cost it brings. A
/// held reservation is released into that cost (the reservation when none
/// is said). A booked one is in the spend already: only a cost above it is
/// booked more, since a real cost is never hidden, and only a reset lowers
/// spend.
pub(crate) fn resolve_in(b: &mut Budget, a: &Action, cost: Option<Micros>) {
    if booked(a) {
        let more = cost.unwrap_or(0).saturating_sub(a.reserved_micros);
        b.spent_micros = b.spent_micros.saturating_add(more);
        return;
    }
    b.held_unknown_micros = b.held_unknown_micros.saturating_sub(a.reserved_micros);
    b.spent_micros = b
        .spent_micros
        .saturating_add(cost.unwrap_or(a.reserved_micros));
}

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
    /// Each one's reservation is booked as spent. A call settled since is
    /// left as it is. The calls it marked.
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
                match k.mark_unknown_as(c, EARLIER_PROCESS, true) {
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
